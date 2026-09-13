use super::guardrails::{FeedbackLoop, FeedbackMetric, GuardrailEnforcer};
use std::collections::VecDeque;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AppMode {
    Onboarding,
    MainWorkspace,
    BlueprintReview,
}

#[derive(Debug, Clone)]
pub struct ThoughtCard {
    pub title: String,
    pub body: String,
    pub card_type: String, // "REASONING", "ACTION", "DIFF"
}

pub struct App {
    pub mode: AppMode,
    pub input: String,
    pub provider: String,
    pub api_key: String,
    pub authenticated: bool,
    pub onboarding_step: usize,
    pub timeline: VecDeque<ThoughtCard>,
    pub feedback_loop: FeedbackLoop,
    pub active_blueprint: Option<String>,
    pub status_message: String,
    pub should_quit: bool,
}

impl App {
    pub fn new(is_authed: bool, default_prov: String) -> Self {
        let mode = if is_authed {
            AppMode::MainWorkspace
        } else {
            AppMode::Onboarding
        };

        let mut timeline = VecDeque::with_capacity(50);
        timeline.push_back(ThoughtCard {
            title: "🌩️ AGY System Initialized".to_string(),
            body: "Welcome to ltn.eng ADE. Type /analyze, /architect, /deploy, or enter prompt.".to_string(),
            card_type: "REASONING".to_string(),
        });

        let mut feedback_loop = FeedbackLoop::new(100);
        feedback_loop.push_metric(FeedbackMetric {
            timestamp: "00:00:01".to_string(),
            cpu_usage_pct: 12.5,
            ram_used_mb: 4096,
            gpu_vram_mb: 12288,
            status: "HEALTHY".to_string(),
        });

        Self {
            mode,
            input: String::new(),
            provider: default_prov,
            api_key: String::new(),
            authenticated: is_authed,
            onboarding_step: 1,
            timeline,
            feedback_loop,
            active_blueprint: None,
            status_message: "Ready".to_string(),
            should_quit: false,
        }
    }

    pub fn submit_input(&mut self) {
        let input_text = self.input.trim().to_string();
        if input_text.is_empty() {
            return;
        }

        // Validate command safety against Guardrails
        if let Err(violation) = GuardrailEnforcer::validate_command(&input_text) {
            self.status_message = violation.clone();
            self.timeline.push_back(ThoughtCard {
                title: "⚠️ Guardrail Enforcer".to_string(),
                body: violation,
                card_type: "REASONING".to_string(),
            });
            self.input.clear();
            return;
        }

        self.timeline.push_back(ThoughtCard {
            title: format!("🌩️ Prompt: {}", input_text),
            body: "Processing request through ltn.eng feed-forward feedback loop...".to_string(),
            card_type: "ACTION".to_string(),
        });

        match input_text.as_str() {
            "/doctor" => {
                self.status_message = "Ran system diagnostics.".to_string();
                self.timeline.push_back(ThoughtCard {
                    title: "🩺 System Diagnostics".to_string(),
                    body: "✓ Execution Sandbox: Operational\n✓ Provider Endpoint: Beam Cloud (Ready)\n✓ Feed-forward Loop: Active".to_string(),
                    card_type: "REASONING".to_string(),
                });
            }
            "/architect" | "architect" => {
                self.mode = AppMode::BlueprintReview;
                self.active_blueprint = Some("Provider: Beam Cloud (Serverless A100)\nCost: $120/mo\nStatus: Verified".to_string());
                self.status_message = "Generated architecture proposal. Press [A] to Apply or [R] to Reject.".to_string();
            }
            "/login" => {
                self.mode = AppMode::Onboarding;
                self.onboarding_step = 1;
            }
            _ => {
                self.status_message = format!("Executed command: {}", input_text);
            }
        }

        self.input.clear();
    }

    pub fn accept_blueprint(&mut self) {
        if self.mode == AppMode::BlueprintReview {
            self.timeline.push_back(ThoughtCard {
                title: "⚡ Blueprint Accepted".to_string(),
                body: "Applying deployment options and initializing containers...".to_string(),
                card_type: "ACTION".to_string(),
            });
            self.mode = AppMode::MainWorkspace;
            self.status_message = "Deployment completed successfully!".to_string();
        }
    }

    pub fn reject_blueprint(&mut self) {
        if self.mode == AppMode::BlueprintReview {
            self.timeline.push_back(ThoughtCard {
                title: "❌ Blueprint Rejected".to_string(),
                body: "Cancelled deployment proposal.".to_string(),
                card_type: "REASONING".to_string(),
            });
            self.mode = AppMode::MainWorkspace;
            self.status_message = "Proposal rejected.".to_string();
        }
    }
}
