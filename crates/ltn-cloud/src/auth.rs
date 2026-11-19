//! AWS credential check through the standard credential chain
//! (`aws sts get-caller-identity`). No key is ever passed in.

use crate::exec::{self, Out};
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    pub account: String,
    pub arn: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStatus {
    Configured(Identity),
    Unconfigured(String),
}

impl AuthStatus {
    pub fn is_configured(&self) -> bool {
        matches!(self, AuthStatus::Configured(_))
    }
}

/// Parses `aws sts get-caller-identity --output json`.
pub fn parse_caller_identity(text: &str) -> Result<Identity> {
    let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("unreadable sts output: {e}"))?;
    let account = v["Account"].as_str().ok_or_else(|| anyhow!("sts output has no Account"))?;
    let arn = v["Arn"].as_str().ok_or_else(|| anyhow!("sts output has no Arn"))?;
    Ok(Identity { account: account.into(), arn: arn.into() })
}

/// Turns the outcome of running the sts call into a status with an actionable reason.
pub fn status_from_output(bin: &str, res: Result<Out>) -> AuthStatus {
    let out = match res {
        Ok(o) => o,
        Err(e) => {
            return AuthStatus::Unconfigured(format!(
                "The AWS CLI could not be started ({e}). Install AWS CLI v2 \
                 (https://aws.amazon.com/cli/) or set LTN_AWS_BIN to its path, then retry."
            ))
        }
    };
    if out.ok {
        return match parse_caller_identity(&out.stdout) {
            Ok(id) => AuthStatus::Configured(id),
            Err(e) => AuthStatus::Unconfigured(format!("`{bin} sts get-caller-identity` succeeded but {e}")),
        };
    }
    let err = out.evidence();
    let lower = err.to_ascii_lowercase();
    let hint = if lower.contains("unable to locate credentials") || lower.contains("no credentials") {
        "No AWS credentials were found in the server's environment. Configure the standard AWS \
         credential chain where the ltn server runs: `aws configure`, `aws sso login --profile <p>` \
         with AWS_PROFILE set, or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY environment variables. \
         Then call ltn_cloud_auth again (restart the server if you changed its environment)."
    } else if lower.contains("expired") {
        "The AWS credentials have expired. Refresh them (e.g. `aws sso login`) and call ltn_cloud_auth again."
    } else if lower.contains("invalidclienttokenid") || lower.contains("signaturedoesnotmatch") {
        "AWS rejected the configured credentials (invalid key or secret). Fix the credential chain \
         configuration and call ltn_cloud_auth again."
    } else if lower.contains("could not connect") || lower.contains("endpoint") {
        "The AWS STS endpoint could not be reached. Check network access and the configured region."
    } else {
        "AWS credentials could not be verified. Run `aws sts get-caller-identity` in the server's \
         environment to see the problem."
    };
    let first = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    AuthStatus::Unconfigured(format!("{hint} (aws said: {first})"))
}

pub fn check(aws_bin: &str) -> AuthStatus {
    let res = exec::run(aws_bin, &["sts", "get-caller-identity", "--output", "json"], None);
    status_from_output(aws_bin, res)
}

/// Region from AWS_REGION, AWS_DEFAULT_REGION, else `aws configure get region`.
pub fn default_region(aws_bin: &str) -> Option<String> {
    for k in ["AWS_REGION", "AWS_DEFAULT_REGION"] {
        if let Some(v) = std::env::var(k).ok().filter(|s| !s.trim().is_empty()) {
            return Some(v.trim().to_string());
        }
    }
    exec::run(aws_bin, &["configure", "get", "region"], None)
        .ok()
        .filter(|o| o.ok && !o.stdout.trim().is_empty())
        .map(|o| o.stdout.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(ok: bool, stdout: &str, stderr: &str) -> Out {
        Out { ok, code: Some(if ok { 0 } else { 255 }), stdout: stdout.into(), stderr: stderr.into() }
    }

    #[test]
    fn configured_identity_is_parsed() {
        let text = include_str!("../tests/fixtures/sts-get-caller-identity.json");
        let st = status_from_output("aws", Ok(out(true, text, "")));
        assert_eq!(
            st,
            AuthStatus::Configured(Identity {
                account: "123456789012".into(),
                arn: "arn:aws:iam::123456789012:user/ltn-doc-example".into()
            })
        );
    }

    #[test]
    fn missing_credentials_give_actionable_reason() {
        let st = status_from_output(
            "aws",
            Ok(out(
                false,
                "",
                "\nUnable to locate credentials. You can configure credentials by running \"aws configure\".",
            )),
        );
        match st {
            AuthStatus::Unconfigured(r) => {
                assert!(r.contains("No AWS credentials"), "{r}");
                assert!(r.contains("aws configure"), "{r}");
                assert!(r.contains("Unable to locate credentials"), "{r}");
            }
            other => panic!("expected unconfigured, got {other:?}"),
        }
    }

    #[test]
    fn expired_and_missing_binary_are_distinguished() {
        let st = status_from_output("aws", Ok(out(false, "", "An error occurred (ExpiredToken): token expired")));
        assert!(matches!(st, AuthStatus::Unconfigured(ref r) if r.contains("expired")), "{st:?}");
        let st = status_from_output("aws", Err(anyhow!("program not found")));
        assert!(matches!(st, AuthStatus::Unconfigured(ref r) if r.contains("LTN_AWS_BIN")), "{st:?}");
        assert!(!check("Z:/definitely/not/aws-cli.exe").is_configured());
    }

    #[test]
    fn garbage_success_output_is_not_configured() {
        let st = status_from_output("aws", Ok(out(true, "not json", "")));
        assert!(!st.is_configured());
    }
}
