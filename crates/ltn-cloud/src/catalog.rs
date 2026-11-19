//! Instance catalog: instance types from `aws ec2 describe-instance-types`
//! joined with on-demand Linux prices from `aws pricing get-products`.
//! There are no built-in prices; the only source is a fetched, cached catalog.

use crate::exec;
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use ltn_state::store::State;
use ltn_state::{Offer, TargetCapacity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const DEFAULT_FAMILIES: &[&str] = &["t3", "t3a", "m6i", "m7i", "c6i", "c7i", "g4dn", "g5"];

/// One instance type as described by EC2 (before prices are joined).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceType {
    pub instance_type: String,
    pub vcpus: u64,
    pub memory_mib: u64,
    pub gpu_count: u64,
    /// Total GPU memory across all GPUs.
    pub gpu_vram_mib: u64,
    pub gpu_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    #[serde(flatten)]
    pub spec: InstanceType,
    /// Exact decimal string from the AWS price list, e.g. "0.0104000000".
    pub usd_per_hour: String,
    /// `usd_per_hour` in whole cents, rounded up.
    pub cents_per_hour: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub region: String,
    pub fetched_at: DateTime<Utc>,
    pub families: Vec<String>,
    pub entries: Vec<CatalogEntry>,
    /// Described instance types for which no matching price was found.
    #[serde(default)]
    pub unpriced: Vec<String>,
}

fn family_of(instance_type: &str) -> &str {
    instance_type.split('.').next().unwrap_or(instance_type)
}

/// Parses `aws ec2 describe-instance-types --output json`, keeping current
/// generation, x86_64, on-demand capable types whose family is in `families`.
pub fn parse_instance_types(text: &str, families: &[String]) -> Result<Vec<InstanceType>> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| anyhow!("unreadable describe-instance-types output: {e}"))?;
    let list = v["InstanceTypes"]
        .as_array()
        .ok_or_else(|| anyhow!("describe-instance-types output has no InstanceTypes array"))?;
    let fams: BTreeSet<&str> = families.iter().map(String::as_str).collect();
    let has = |arr: &Value, want: &str| {
        arr.as_array().map_or(false, |a| a.iter().any(|x| x.as_str() == Some(want)))
    };
    let mut out = Vec::new();
    for it in list {
        let Some(name) = it["InstanceType"].as_str() else { continue };
        if !fams.is_empty() && !fams.contains(family_of(name)) {
            continue;
        }
        if it["CurrentGeneration"].as_bool() != Some(true)
            || !has(&it["ProcessorInfo"]["SupportedArchitectures"], "x86_64")
            || !has(&it["SupportedUsageClasses"], "on-demand")
        {
            continue;
        }
        let vcpus = it["VCpuInfo"]["DefaultVCpus"].as_u64().unwrap_or(0);
        let memory_mib = it["MemoryInfo"]["SizeInMiB"].as_u64().unwrap_or(0);
        if vcpus == 0 || memory_mib == 0 {
            continue;
        }
        let gpus = it["GpuInfo"]["Gpus"].as_array().cloned().unwrap_or_default();
        let gpu_count: u64 = gpus.iter().map(|g| g["Count"].as_u64().unwrap_or(0)).sum();
        let per_gpu_total: u64 = gpus
            .iter()
            .map(|g| g["Count"].as_u64().unwrap_or(0) * g["MemoryInfo"]["SizeInMiB"].as_u64().unwrap_or(0))
            .sum();
        let gpu_vram_mib = it["GpuInfo"]["TotalGpuMemoryInMiB"].as_u64().unwrap_or(per_gpu_total);
        let gpu_name = gpus.first().and_then(|g| g["Name"].as_str()).map(str::to_string);
        out.push(InstanceType {
            instance_type: name.to_string(),
            vcpus,
            memory_mib,
            gpu_count,
            gpu_vram_mib,
            gpu_name,
        });
    }
    out.sort_by(|a, b| a.instance_type.cmp(&b.instance_type));
    Ok(out)
}

/// Parses `aws pricing get-products --service-code AmazonEC2 --output json`
/// (each `PriceList` element is itself a JSON document, as a string). Keeps
/// Linux, shared tenancy, no pre-installed software, capacitystatus Used,
/// in `region`, and returns instance type -> on-demand USD per hour string.
pub fn parse_prices(text: &str, region: &str) -> Result<BTreeMap<String, String>> {
    let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("unreadable get-products output: {e}"))?;
    let list = v["PriceList"].as_array().ok_or_else(|| anyhow!("get-products output has no PriceList array"))?;
    let mut out = BTreeMap::new();
    for item in list {
        // The CLI returns strings; accept already-decoded objects too.
        let doc: Value = match item {
            Value::String(s) => match serde_json::from_str(s) {
                Ok(d) => d,
                Err(_) => continue,
            },
            other => other.clone(),
        };
        let a = &doc["product"]["attributes"];
        let attr = |k: &str| a[k].as_str().unwrap_or("");
        if attr("instanceType").is_empty()
            || attr("operatingSystem") != "Linux"
            || attr("tenancy") != "Shared"
            || attr("preInstalledSw") != "NA"
            || attr("capacitystatus") != "Used"
            || (!attr("regionCode").is_empty() && attr("regionCode") != region)
        {
            continue;
        }
        let Some(terms) = doc["terms"]["OnDemand"].as_object() else { continue };
        let price = terms
            .values()
            .filter_map(|t| t["priceDimensions"].as_object())
            .flat_map(|d| d.values())
            .filter(|d| d["unit"].as_str() == Some("Hrs"))
            .filter_map(|d| d["pricePerUnit"]["USD"].as_str())
            .find(|p| usd_to_cents_ceil(p).map_or(false, |c| c > 0));
        if let Some(p) = price {
            out.entry(attr("instanceType").to_string()).or_insert_with(|| p.to_string());
        }
    }
    Ok(out)
}

/// Exact decimal USD string -> whole cents, rounded up. "0.0104" -> 2, "0.10" -> 10.
pub fn usd_to_cents_ceil(usd: &str) -> Option<u64> {
    let s = usd.trim();
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    if !int.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let dollars: u64 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let cents_digits: String = frac.chars().chain(std::iter::repeat('0')).take(2).collect();
    let mut cents = dollars.checked_mul(100)? + cents_digits.parse::<u64>().ok()?;
    if frac.len() > 2 && frac[2..].chars().any(|c| c != '0') {
        cents += 1;
    }
    Some(cents)
}

/// Joins described types with prices. Types without a price are listed as unpriced.
pub fn join(types: Vec<InstanceType>, prices: &BTreeMap<String, String>) -> (Vec<CatalogEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut unpriced = Vec::new();
    for t in types {
        match prices.get(&t.instance_type).and_then(|p| usd_to_cents_ceil(p).map(|c| (p, c))) {
            Some((p, c)) => entries.push(CatalogEntry { spec: t, usd_per_hour: p.clone(), cents_per_hour: c }),
            None => unpriced.push(t.instance_type),
        }
    }
    (entries, unpriced)
}

pub fn offer_id(instance_type: &str, region: &str) -> String {
    format!("aws:{instance_type}@{region}")
}

/// `aws:<type>@<region>` -> (type, region).
pub fn parse_offer_id(id: &str) -> Result<(String, String)> {
    let rest = id.strip_prefix("aws:").ok_or_else(|| anyhow!("offer `{id}` is not an AWS offer (expected aws:<type>@<region>)"))?;
    let (t, r) = rest.split_once('@').ok_or_else(|| anyhow!("offer `{id}` has no region (expected aws:<type>@<region>)"))?;
    if t.is_empty() || r.is_empty() {
        bail!("offer `{id}` is malformed (expected aws:<type>@<region>)");
    }
    Ok((t.to_string(), r.to_string()))
}

pub fn to_offer(entry: &CatalogEntry, region: &str, fetched_at: &DateTime<Utc>) -> Offer {
    let id = offer_id(&entry.spec.instance_type, region);
    let gpu = match (&entry.spec.gpu_name, entry.spec.gpu_count) {
        (Some(n), c) if c > 0 => format!(", {c}x {n} ({} MiB VRAM)", entry.spec.gpu_vram_mib),
        _ => String::new(),
    };
    Offer {
        offer_id: id.clone(),
        provider: "aws".into(),
        description: format!(
            "AWS {} in {region}: {} vCPU, {} MiB RAM{gpu}; on-demand Linux ${}/h (price list fetched {})",
            entry.spec.instance_type,
            entry.spec.vcpus,
            entry.spec.memory_mib,
            entry.usd_per_hour.trim_end_matches('0').trim_end_matches('.'),
            fetched_at.to_rfc3339()
        ),
        capacity: TargetCapacity {
            target_id: id,
            free_cpu_millis: entry.spec.vcpus * 1000,
            free_ram_mb: entry.spec.memory_mib,
            free_gpu_count: entry.spec.gpu_count,
            free_vram_mb: entry.spec.gpu_vram_mib,
            cost_cents_per_hour: entry.cents_per_hour,
        },
    }
}

// ---- cache in State.ext["cloud"]["catalogs"][<region>] ----

pub fn cached_catalogs(state: &State) -> Vec<Catalog> {
    state
        .ext
        .get("cloud")
        .and_then(|c| c.get("catalogs"))
        .and_then(Value::as_object)
        .map(|m| m.values().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
        .unwrap_or_default()
}

pub fn cached_catalog(state: &State, region: &str) -> Option<Catalog> {
    cached_catalogs(state).into_iter().find(|c| c.region == region)
}

pub fn store_catalog(state: &mut State, catalog: &Catalog) -> Result<()> {
    let cloud = state.ext.entry("cloud".into()).or_insert_with(|| serde_json::json!({}));
    if !cloud.is_object() {
        *cloud = serde_json::json!({});
    }
    let catalogs = cloud
        .as_object_mut()
        .expect("object")
        .entry("catalogs")
        .or_insert_with(|| serde_json::json!({}));
    if !catalogs.is_object() {
        *catalogs = serde_json::json!({});
    }
    catalogs.as_object_mut().expect("object").insert(catalog.region.clone(), serde_json::to_value(catalog)?);
    Ok(())
}

/// All offers from every cached catalog. Pure: no credentials check here.
pub fn offers_from_state(state: &State) -> Vec<Offer> {
    let mut out = Vec::new();
    for cat in cached_catalogs(state) {
        for e in &cat.entries {
            out.push(to_offer(e, &cat.region, &cat.fetched_at));
        }
    }
    out
}

pub fn families_from_env() -> Vec<String> {
    match std::env::var("LTN_AWS_FAMILIES") {
        Ok(s) if !s.trim().is_empty() => {
            s.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect()
        }
        _ => DEFAULT_FAMILIES.iter().map(|s| s.to_string()).collect(),
    }
}

/// Fetches both lists from AWS (read-only calls) and joins them.
pub fn fetch(aws_bin: &str, region: &str, families: &[String]) -> Result<Catalog> {
    if families.is_empty() {
        bail!("the instance family allowlist is empty");
    }
    let type_filter = format!(
        "Name=instance-type,Values={}",
        families.iter().map(|f| format!("{f}.*")).collect::<Vec<_>>().join(",")
    );
    let desc = exec::run(
        aws_bin,
        &[
            "ec2",
            "describe-instance-types",
            "--region",
            region,
            "--filters",
            "Name=current-generation,Values=true",
            "Name=processor-info.supported-architecture,Values=x86_64",
            "Name=supported-usage-class,Values=on-demand",
            &type_filter,
            "--output",
            "json",
        ],
        None,
    )?;
    if !desc.ok {
        bail!("`aws ec2 describe-instance-types` failed in {region}: {}", desc.evidence());
    }
    let types = parse_instance_types(&desc.stdout, families)?;
    if types.is_empty() {
        bail!("`aws ec2 describe-instance-types` returned no instance types for families {families:?} in {region}");
    }
    let region_filter = format!("Type=TERM_MATCH,Field=regionCode,Value={region}");
    let prices = exec::run(
        aws_bin,
        &[
            "pricing",
            "get-products",
            "--region",
            "us-east-1",
            "--service-code",
            "AmazonEC2",
            "--filters",
            &region_filter,
            "Type=TERM_MATCH,Field=operatingSystem,Value=Linux",
            "Type=TERM_MATCH,Field=tenancy,Value=Shared",
            "Type=TERM_MATCH,Field=preInstalledSw,Value=NA",
            "Type=TERM_MATCH,Field=capacitystatus,Value=Used",
            "--output",
            "json",
        ],
        None,
    )?;
    if !prices.ok {
        bail!("`aws pricing get-products` failed: {}", prices.evidence());
    }
    let price_map = parse_prices(&prices.stdout, region)?;
    let (entries, unpriced) = join(types, &price_map);
    if entries.is_empty() {
        bail!("no prices matched the described instance types in {region}; unpriced: {unpriced:?}");
    }
    Ok(Catalog { region: region.into(), fetched_at: Utc::now(), families: families.to_vec(), entries, unpriced })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures in tests/fixtures/ were written from the documented AWS CLI
    // response formats; they were NOT captured live (no AWS credentials were
    // configured on the development machine). 123456789012 is AWS's doc example account.
    const DESC: &str = include_str!("../tests/fixtures/ec2-describe-instance-types.json");
    const PRICES: &str = include_str!("../tests/fixtures/pricing-get-products.json");

    fn fams() -> Vec<String> {
        DEFAULT_FAMILIES.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn instance_types_parser_filters_and_reads_gpus() {
        let types = parse_instance_types(DESC, &fams()).unwrap();
        let names: Vec<&str> = types.iter().map(|t| t.instance_type.as_str()).collect();
        // m4 (previous gen / not allowlisted), a1 (arm64), t3a.nano (spot only) dropped.
        assert_eq!(names, vec!["c7i.large", "g4dn.xlarge", "g5.12xlarge", "m6i.large", "t3.micro"]);
        let g5 = types.iter().find(|t| t.instance_type == "g5.12xlarge").unwrap();
        assert_eq!((g5.vcpus, g5.memory_mib, g5.gpu_count, g5.gpu_vram_mib), (48, 196608, 4, 98304));
        assert_eq!(g5.gpu_name.as_deref(), Some("A10G"));
        let t3 = types.iter().find(|t| t.instance_type == "t3.micro").unwrap();
        assert_eq!((t3.vcpus, t3.memory_mib, t3.gpu_count, t3.gpu_vram_mib), (2, 1024, 0, 0));
        // Allowlist is honoured.
        let only_t3 = parse_instance_types(DESC, &["t3".to_string()]).unwrap();
        assert_eq!(only_t3.len(), 1);
        assert!(parse_instance_types("{}", &fams()).is_err());
    }

    #[test]
    fn prices_parser_keeps_linux_shared_used_only() {
        let p = parse_prices(PRICES, "us-east-1").unwrap();
        // The zero-priced capacity reservation row and the Windows row are ignored.
        assert_eq!(p.get("t3.micro").map(String::as_str), Some("0.0104000000"));
        assert_eq!(p.get("m6i.large").map(String::as_str), Some("0.0960000000"));
        assert_eq!(p.get("g4dn.xlarge").map(String::as_str), Some("0.5260000000"));
        assert!(p.contains_key("r5.large"));
        assert!(parse_prices(PRICES, "eu-west-1").unwrap().is_empty());
        assert!(parse_prices("[]", "us-east-1").is_err());
    }

    #[test]
    fn cents_round_up_exactly() {
        assert_eq!(usd_to_cents_ceil("0.0104000000"), Some(2));
        assert_eq!(usd_to_cents_ceil("0.1000000000"), Some(10));
        assert_eq!(usd_to_cents_ceil("0.0960000000"), Some(10));
        assert_eq!(usd_to_cents_ceil("0.0892500000"), Some(9));
        assert_eq!(usd_to_cents_ceil("5.6720000000"), Some(568));
        assert_eq!(usd_to_cents_ceil("3"), Some(300));
        assert_eq!(usd_to_cents_ceil("0.0000000000"), Some(0));
        assert_eq!(usd_to_cents_ceil("abc"), None);
        assert_eq!(usd_to_cents_ceil("-1.0"), None);
    }

    #[test]
    fn join_and_offer_mapping() {
        let types = parse_instance_types(DESC, &fams()).unwrap();
        let prices = parse_prices(PRICES, "us-east-1").unwrap();
        let (entries, unpriced) = join(types, &prices);
        assert_eq!(entries.len(), 5);
        assert!(unpriced.is_empty());
        let fetched = Utc::now();
        let g4 = entries.iter().find(|e| e.spec.instance_type == "g4dn.xlarge").unwrap();
        let o = to_offer(g4, "us-east-1", &fetched);
        assert_eq!(o.offer_id, "aws:g4dn.xlarge@us-east-1");
        assert_eq!(o.capacity.target_id, o.offer_id);
        assert_eq!(o.provider, "aws");
        assert_eq!(o.capacity.free_cpu_millis, 4000);
        assert_eq!(o.capacity.free_ram_mb, 16384);
        assert_eq!(o.capacity.free_gpu_count, 1);
        assert_eq!(o.capacity.free_vram_mb, 16384);
        assert_eq!(o.capacity.cost_cents_per_hour, 53); // 0.526 -> 53
        assert!(o.description.contains("T4"));
        let t3 = entries.iter().find(|e| e.spec.instance_type == "t3.micro").unwrap();
        assert_eq!(to_offer(t3, "us-east-1", &fetched).capacity.cost_cents_per_hour, 2); // 1.04 -> 2
        assert_eq!(parse_offer_id("aws:g4dn.xlarge@us-east-1").unwrap(), ("g4dn.xlarge".into(), "us-east-1".into()));
        assert!(parse_offer_id("gcp:x@y").is_err());
        assert!(parse_offer_id("aws:t3.micro").is_err());
    }

    #[test]
    fn offers_empty_without_cache_and_present_with_it() {
        let mut st = State::default();
        assert!(offers_from_state(&st).is_empty());
        let types = parse_instance_types(DESC, &fams()).unwrap();
        let (entries, unpriced) = join(types, &parse_prices(PRICES, "us-east-1").unwrap());
        let cat = Catalog { region: "us-east-1".into(), fetched_at: Utc::now(), families: fams(), entries, unpriced };
        store_catalog(&mut st, &cat).unwrap();
        let back = cached_catalog(&st, "us-east-1").unwrap();
        assert_eq!(back, cat);
        assert_eq!(offers_from_state(&st).len(), 5);
        // Round-trips through JSON (the store's format).
        let st2: State = serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
        assert_eq!(offers_from_state(&st2).len(), 5);
    }
}
