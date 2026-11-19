use ltn_architect::classifier::ProjectClassification;
use ltn_core::ir::WorkloadKind;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedIacPackage {
    pub project_name: String,
    pub provider: String,
    pub dockerfile: String,
    pub docker_compose: String,
    pub kubernetes_yaml: String,
    pub cloud_manifest: String,
    pub deploy_script_sh: String,
    pub deploy_script_ps1: String,
}

impl GeneratedIacPackage {
    /// Writes all generated declarative IaC files into the destination directory.
    pub fn write_to_dir(&self, base_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let deploy_dir = base_dir.join(".ltn").join("deploy");
        fs::create_dir_all(&deploy_dir)?;
        let mut written = Vec::new();

        let df_path = deploy_dir.join("Dockerfile");
        fs::write(&df_path, &self.dockerfile)?;
        written.push(df_path);

        let dc_path = deploy_dir.join("docker-compose.yml");
        fs::write(&dc_path, &self.docker_compose)?;
        written.push(dc_path);

        let k8s_path = deploy_dir.join("kubernetes.yaml");
        fs::write(&k8s_path, &self.kubernetes_yaml)?;
        written.push(k8s_path);

        let manifest_filename = match self.provider.to_uppercase().as_str() {
            "AWS" => "aws-ecs-task.json",
            "GCP" => "gcp-cloudrun.yaml",
            "AZURE" => "azure-containerapp.json",
            _ => "cloud-spec.json",
        };
        let cloud_path = deploy_dir.join(manifest_filename);
        fs::write(&cloud_path, &self.cloud_manifest)?;
        written.push(cloud_path);

        let sh_path = deploy_dir.join("deploy.sh");
        fs::write(&sh_path, &self.deploy_script_sh)?;
        written.push(sh_path);

        let ps1_path = deploy_dir.join("deploy.ps1");
        fs::write(&ps1_path, &self.deploy_script_ps1)?;
        written.push(ps1_path);

        Ok(written)
    }
}

pub struct IacGenerator;

impl IacGenerator {
    pub fn generate_package(
        classification: &ProjectClassification,
        target_provider: &str,
    ) -> GeneratedIacPackage {
        let project_name = classification
            .description
            .split_whitespace()
            .next()
            .unwrap_or("app")
            .to_lowercase();
        let provider = target_provider.to_uppercase();

        let (dockerfile, docker_compose, kubernetes_yaml) = match classification.workload_kind {
            WorkloadKind::NodePostgresApi => (
                Self::node_postgres_dockerfile(),
                Self::node_postgres_compose(),
                Self::node_postgres_k8s(classification),
            ),
            WorkloadKind::PythonVllmGpu => (
                Self::python_vllm_dockerfile(),
                Self::python_vllm_compose(),
                Self::python_vllm_k8s(classification),
            ),
            WorkloadKind::StaticWeb => (
                Self::static_web_dockerfile(),
                Self::static_web_compose(),
                Self::static_web_k8s(classification),
            ),
            WorkloadKind::GoWorker => (
                Self::go_worker_dockerfile(),
                Self::go_worker_compose(),
                Self::go_worker_k8s(classification),
            ),
            _ => (
                Self::generic_dockerfile(),
                Self::generic_compose(),
                Self::generic_k8s(classification),
            ),
        };

        let cloud_manifest = match provider.as_str() {
            "AWS" => Self::aws_ecs_manifest(&project_name, classification),
            "GCP" => Self::gcp_cloudrun_manifest(&project_name, classification),
            "AZURE" => Self::azure_containerapp_manifest(&project_name, classification),
            _ => Self::docker_local_manifest(&project_name, classification),
        };

        let deploy_script_sh = Self::generate_deploy_sh(&provider, &project_name);
        let deploy_script_ps1 = Self::generate_deploy_ps1(&provider, &project_name);

        GeneratedIacPackage {
            project_name,
            provider,
            dockerfile,
            docker_compose,
            kubernetes_yaml,
            cloud_manifest,
            deploy_script_sh,
            deploy_script_ps1,
        }
    }

    fn node_postgres_dockerfile() -> String {
        r#"# Multi-Stage Node.js + PostgreSQL Production Dockerfile
FROM node:20-alpine AS builder
WORKDIR /app
COPY package*.json ./
RUN npm ci --omit=dev

FROM node:20-alpine AS runner
WORKDIR /app
ENV NODE_ENV=production
RUN addgroup -S appgroup && adduser -S appuser -G appgroup
COPY --from=builder /app/node_modules ./node_modules
COPY . .
USER appuser
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD wget --quiet --tries=1 --spider http://localhost:3000/health || exit 1
CMD ["node", "index.js"]
"#
        .to_string()
    }

    fn node_postgres_compose() -> String {
        r#"version: '3.8'

services:
  api:
    build:
      context: .
      dockerfile: Dockerfile
    restart: unless-stopped
    ports:
      - "3000:3000"
    environment:
      - NODE_ENV=production
      - DATABASE_URL=postgres://postgres:postgres@db:5432/app_db
    depends_on:
      db:
        condition: service_healthy
    networks:
      - ltn-net

  db:
    image: postgres:16-alpine
    restart: unless-stopped
    ports:
      - "5432:5432"
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: app_db
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks:
      - ltn-net

networks:
  ltn-net:
    driver: bridge

volumes:
  pgdata:
"#
        .to_string()
    }

    fn node_postgres_k8s(c: &ProjectClassification) -> String {
        format!(
            r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: node-api-deployment
  labels:
    app: node-api
spec:
  replicas: 2
  selector:
    matchLabels:
      app: node-api
  template:
    metadata:
      labels:
        app: node-api
    spec:
      containers:
      - name: api
        image: ltn/node-api:latest
        ports:
        - containerPort: 3000
        resources:
          requests:
            cpu: "{}m"
            memory: "{}Mi"
          limits:
            cpu: "{}m"
            memory: "{}Mi"
        livenessProbe:
          httpGet:
            path: /health
            port: 3000
          initialDelaySeconds: 15
          periodSeconds: 20
---
apiVersion: v1
kind: Service
metadata:
  name: node-api-service
spec:
  type: ClusterIP
  ports:
  - port: 80
    targetPort: 3000
  selector:
    app: node-api
"#,
            c.recommended_cpu_cores * 500,
            c.recommended_ram_mb / 2,
            c.recommended_cpu_cores * 1000,
            c.recommended_ram_mb
        )
    }

    fn python_vllm_dockerfile() -> String {
        r#"# Multi-Stage Python vLLM GPU Inference Dockerfile
FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04 AS runtime
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip python3-dev build-essential curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
COPY requirements.txt ./
RUN pip3 install --no-cache-dir -r requirements.txt
COPY . .
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
  CMD curl -f http://localhost:8000/health || exit 1
CMD ["python3", "-m", "vllm.entrypoints.openai.api_server", "--port", "8000"]
"#
        .to_string()
    }

    fn python_vllm_compose() -> String {
        r#"version: '3.8'

services:
  vllm-inference:
    build:
      context: .
      dockerfile: Dockerfile
    ports:
      - "8000:8000"
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: all
              capabilities: [gpu]
    environment:
      - MODEL_NAME=deepseek-ai/DeepSeek-Coder
      - GPU_MEMORY_UTILIZATION=0.90
    restart: unless-stopped
"#
        .to_string()
    }

    fn python_vllm_k8s(c: &ProjectClassification) -> String {
        format!(
            r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: vllm-gpu-deployment
spec:
  replicas: 1
  selector:
    matchLabels:
      app: vllm-inference
  template:
    metadata:
      labels:
        app: vllm-inference
    spec:
      containers:
      - name: vllm
        image: vllm/vllm-openai:latest
        resources:
          limits:
            cpu: "{}"
            memory: "{}Mi"
            nvidia.com/gpu: "{}"
        ports:
        - containerPort: 8000
"#,
            c.recommended_cpu_cores,
            c.recommended_ram_mb,
            c.recommended_gpu_count.max(1)
        )
    }

    fn static_web_dockerfile() -> String {
        r#"# Static Web Production Dockerfile
FROM node:20-alpine AS build
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM nginx:alpine
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
"#
        .to_string()
    }

    fn static_web_compose() -> String {
        r#"version: '3.8'
services:
  web:
    build: .
    ports:
      - "80:80"
    restart: always
"#
        .to_string()
    }

    fn static_web_k8s(_c: &ProjectClassification) -> String {
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: static-web-deployment
spec:
  replicas: 2
  selector:
    matchLabels:
      app: static-web
  template:
    metadata:
      labels:
        app: static-web
    spec:
      containers:
      - name: web
        image: ltn/static-web:latest
        ports:
        - containerPort: 80
"#
        .to_string()
    }

    fn go_worker_dockerfile() -> String {
        r#"# Go Minimal Production Dockerfile
FROM golang:1.22-alpine AS builder
WORKDIR /app
COPY go.* ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o /app/worker .

FROM scratch
COPY --from=builder /app/worker /worker
ENTRYPOINT ["/worker"]
"#
        .to_string()
    }

    fn go_worker_compose() -> String {
        r#"version: '3.8'
services:
  worker:
    build: .
    restart: always
"#
        .to_string()
    }

    fn go_worker_k8s(_c: &ProjectClassification) -> String {
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: go-worker-deployment
spec:
  replicas: 2
  template:
    spec:
      containers:
      - name: worker
        image: ltn/go-worker:latest
"#
        .to_string()
    }

    fn generic_dockerfile() -> String {
        r#"FROM alpine:latest
WORKDIR /app
COPY . .
EXPOSE 8080
CMD ["echo", "Container ready"]
"#
        .to_string()
    }

    fn generic_compose() -> String {
        r#"version: '3.8'
services:
  app:
    build: .
    ports:
      - "8080:8080"
"#
        .to_string()
    }

    fn generic_k8s(_c: &ProjectClassification) -> String {
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: generic-app
spec:
  replicas: 1
  template:
    spec:
      containers:
      - name: app
        image: ltn/generic-app:latest
"#
        .to_string()
    }

    fn aws_ecs_manifest(name: &str, c: &ProjectClassification) -> String {
        serde_json::json!({
            "family": format!("{}-task", name),
            "networkMode": "awsvpc",
            "requiresCompatibilities": ["FARGATE"],
            "cpu": format!("{}", c.recommended_cpu_cores * 1024),
            "memory": format!("{}", c.recommended_ram_mb),
            "containerDefinitions": [
                {
                    "name": name,
                    "image": format!("123456789012.dkr.ecr.us-east-1.amazonaws.com/{}:latest", name),
                    "essential": true,
                    "portMappings": [
                        { "containerPort": 8080, "hostPort": 8080, "protocol": "tcp" }
                    ],
                    "logConfiguration": {
                        "logDriver": "awslogs",
                        "options": {
                            "awslogs-group": format!("/ecs/{}", name),
                            "awslogs-region": "us-east-1",
                            "awslogs-stream-prefix": "ecs"
                        }
                    }
                }
            ]
        }).to_string()
    }

    fn gcp_cloudrun_manifest(name: &str, c: &ProjectClassification) -> String {
        format!(
            r#"apiVersion: serving.knative.dev/v1
kind: Service
metadata:
  name: {}
  annotations:
    run.googleapis.com/launch-stage: GA
spec:
  template:
    metadata:
      annotations:
        autoscaling.knative.dev/maxScale: "10"
    spec:
      containers:
      - image: gcr.io/ltn-project/{}:latest
        resources:
          limits:
            cpu: "{}"
            memory: "{}Mi"
        ports:
        - containerPort: 8080
"#,
            name, name, c.recommended_cpu_cores, c.recommended_ram_mb
        )
    }

    fn azure_containerapp_manifest(name: &str, c: &ProjectClassification) -> String {
        serde_json::json!({
            "type": "Microsoft.App/containerApps",
            "name": name,
            "properties": {
                "configuration": {
                    "ingress": {
                        "external": true,
                        "targetPort": 8080
                    }
                },
                "template": {
                    "containers": [
                        {
                            "name": name,
                            "image": format!("ltnregistry.azurecr.io/{}:latest", name),
                            "resources": {
                                "cpu": c.recommended_cpu_cores as f64,
                                "memory": format!("{}Gi", (c.recommended_ram_mb / 1024).max(1))
                            }
                        }
                    ],
                    "scale": {
                        "minReplicas": 1,
                        "maxReplicas": 10
                    }
                }
            }
        })
        .to_string()
    }

    fn docker_local_manifest(name: &str, c: &ProjectClassification) -> String {
        serde_json::json!({
            "target": "LOCAL_DOCKER",
            "name": name,
            "cpu_limit": c.recommended_cpu_cores,
            "ram_limit_mb": c.recommended_ram_mb,
            "network": "bridge",
            "ports": ["8080:8080"]
        })
        .to_string()
    }

    fn generate_deploy_sh(provider: &str, name: &str) -> String {
        match provider.to_uppercase().as_str() {
            "AWS" => format!(
                r#"#!/usr/bin/env bash
set -e
echo "⚡ Deploying '{}' to AWS ECS Fargate..."
aws sts get-caller-identity >/dev/null || (echo "❌ AWS credentials missing"; exit 1)
aws ecs register-task-definition --cli-input-json file://aws-ecs-task.json
echo "🎉 AWS ECS Task registered successfully!"
"#,
                name
            ),
            "GCP" => format!(
                r#"#!/usr/bin/env bash
set -e
echo "⚡ Deploying '{}' to Google Cloud Run..."
gcloud auth print-access-token >/dev/null || (echo "❌ GCP credentials missing"; exit 1)
gcloud run services replace gcp-cloudrun.yaml
echo "🎉 GCP Cloud Run service updated successfully!"
"#,
                name
            ),
            "AZURE" => format!(
                r#"#!/usr/bin/env bash
set -e
echo "⚡ Deploying '{}' to Azure Container Apps..."
az account show >/dev/null || (echo "❌ Azure credentials missing"; exit 1)
az containerapp create --name "{}" --resource-group "ltn-rg" --yaml azure-containerapp.json
echo "🎉 Azure Container App created successfully!"
"#,
                name, name
            ),
            _ => format!(
                r#"#!/usr/bin/env bash
set -e
echo "⚡ Deploying '{}' locally via Docker Compose..."
docker compose up -d --build
echo "🎉 Local Docker deployment operational!"
"#,
                name
            ),
        }
    }

    fn generate_deploy_ps1(provider: &str, name: &str) -> String {
        match provider.to_uppercase().as_str() {
            "AWS" => format!(
                r#"Write-Host "⚡ Deploying '{}' to AWS ECS Fargate..." -ForegroundColor Cyan
aws sts get-caller-identity | Out-Null
aws ecs register-task-definition --cli-input-json file://aws-ecs-task.json
Write-Host "🎉 AWS ECS Task registered successfully!" -ForegroundColor Green
"#,
                name
            ),
            "GCP" => format!(
                r#"Write-Host "⚡ Deploying '{}' to Google Cloud Run..." -ForegroundColor Cyan
gcloud auth print-access-token | Out-Null
gcloud run services replace gcp-cloudrun.yaml
Write-Host "🎉 GCP Cloud Run service updated successfully!" -ForegroundColor Green
"#,
                name
            ),
            "AZURE" => format!(
                r#"Write-Host "⚡ Deploying '{}' to Azure Container Apps..." -ForegroundColor Cyan
az account show | Out-Null
az containerapp create --name "{}" --resource-group "ltn-rg" --yaml azure-containerapp.json
Write-Host "🎉 Azure Container App created successfully!" -ForegroundColor Green
"#,
                name, name
            ),
            _ => format!(
                r#"Write-Host "⚡ Deploying '{}' locally via Docker Compose..." -ForegroundColor Cyan
docker compose up -d --build
Write-Host "🎉 Local Docker deployment operational!" -ForegroundColor Green
"#,
                name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_classification(workload_kind: WorkloadKind) -> ProjectClassification {
        ProjectClassification {
            workload_kind,
            description: "Test App".to_string(),
            recommended_cpu_cores: 2,
            recommended_ram_mb: 2048,
            recommended_vram_mb: 0,
            recommended_gpu_count: 0,
            provenance: None,
        }
    }

    #[test]
    fn test_iac_generator_aws_ecs() {
        let cls = dummy_classification(WorkloadKind::NodePostgresApi);
        let pkg = IacGenerator::generate_package(&cls, "AWS");
        assert_eq!(pkg.provider, "AWS");
        assert!(pkg.dockerfile.contains("node:20-alpine"));
        assert!(pkg.docker_compose.contains("postgres"));
        assert!(pkg.cloud_manifest.contains("FARGATE"));
        assert!(pkg.cloud_manifest.contains("containerDefinitions"));
        assert!(pkg.deploy_script_sh.contains("aws ecs"));
        assert!(pkg.deploy_script_ps1.contains("aws ecs"));
    }

    #[test]
    fn test_iac_generator_gcp_cloudrun() {
        let cls = dummy_classification(WorkloadKind::PythonVllmGpu);
        let pkg = IacGenerator::generate_package(&cls, "GCP");
        assert_eq!(pkg.provider, "GCP");
        assert!(pkg.dockerfile.contains("nvidia/cuda"));
        assert!(pkg.cloud_manifest.contains("serving.knative.dev/v1"));
        assert!(pkg.deploy_script_sh.contains("gcloud run"));
    }

    #[test]
    fn test_iac_generator_azure_containerapp() {
        let cls = dummy_classification(WorkloadKind::StaticWeb);
        let pkg = IacGenerator::generate_package(&cls, "AZURE");
        assert_eq!(pkg.provider, "AZURE");
        assert!(pkg.dockerfile.contains("nginx:alpine"));
        assert!(pkg.cloud_manifest.contains("Microsoft.App/containerApps"));
        assert!(pkg.deploy_script_sh.contains("az containerapp create"));
    }

    #[test]
    fn test_iac_package_write_to_dir() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ltn_test_iac_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cls = dummy_classification(WorkloadKind::GoWorker);
        let pkg = IacGenerator::generate_package(&cls, "AWS");
        let written = pkg.write_to_dir(&temp_dir).expect("write failed");
        assert_eq!(written.len(), 6);
        assert!(temp_dir.join(".ltn/deploy/Dockerfile").exists());
        assert!(temp_dir.join(".ltn/deploy/docker-compose.yml").exists());
        assert!(temp_dir.join(".ltn/deploy/kubernetes.yaml").exists());
        assert!(temp_dir.join(".ltn/deploy/aws-ecs-task.json").exists());
        assert!(temp_dir.join(".ltn/deploy/deploy.sh").exists());
        assert!(temp_dir.join(".ltn/deploy/deploy.ps1").exists());
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
