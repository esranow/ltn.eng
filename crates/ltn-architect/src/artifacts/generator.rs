use super::super::classifier::ProjectClassification;
use ltn_core::ir::WorkloadKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedArtifact {
    pub file_path: String,
    pub artifact_type: String,
    pub content: String,
}

pub struct MissingArtifactGenerator;

impl MissingArtifactGenerator {
    pub fn generate_missing_artifacts(
        classification: &ProjectClassification,
        has_dockerfile: bool,
    ) -> Vec<GeneratedArtifact> {
        let mut artifacts = Vec::new();

        if !has_dockerfile {
            let dockerfile_content = match classification.workload_kind {
                WorkloadKind::PythonVllmGpu => {
                    "FROM vllm/vllm-openai:latest\nEXPOSE 8000\nCMD [\"--model\", \"vllm-lmsys/fastchat-t5-3b-v1.0\"]"
                }
                WorkloadKind::StaticWeb => {
                    "FROM nginx:alpine\nCOPY ./dist /usr/share/nginx/html\nEXPOSE 80"
                }
                WorkloadKind::NodePostgresApi => {
                    "FROM node:20-alpine\nWORKDIR /app\nCOPY package*.json ./\nRUN npm install\nCOPY . .\nEXPOSE 3000\nCMD [\"npm\", \"start\"]"
                }
                WorkloadKind::GoWorker => {
                    "FROM golang:1.22-alpine AS builder\nWORKDIR /app\nCOPY . .\nRUN go build -o main .\nFROM alpine:latest\nCOPY --from=builder /app/main /main\nCMD [\"/main\"]"
                }
                WorkloadKind::RustWasm => {
                    "FROM rust:latest AS builder\nWORKDIR /app\nCOPY . .\nRUN cargo build --release --target wasm32-unknown-unknown"
                }
                WorkloadKind::StatefulStore => {
                    "FROM postgres:16-alpine\nENV POSTGRES_DB=app\nEXPOSE 5432"
                }
                _ => {
                    "FROM alpine:latest\nCMD [\"echo\", \"LTN.ENG Container Live\"]"
                }
            };

            artifacts.push(GeneratedArtifact {
                file_path: "Dockerfile".to_string(),
                artifact_type: "DOCKERFILE".to_string(),
                content: dockerfile_content.to_string(),
            });
        }

        artifacts.push(GeneratedArtifact {
            file_path: "docker-compose.yml".to_string(),
            artifact_type: "DOCKER_COMPOSE".to_string(),
            content: "version: '3.8'\nservices:\n  app:\n    build: .\n    ports:\n      - '8080:8080'\n".to_string(),
        });

        artifacts
    }
}
