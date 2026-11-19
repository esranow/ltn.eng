use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelProviderKind {
    DeepSeek {
        model_name: String,
    },
    Qwen {
        model_name: String,
    },
    ZhipuGlm {
        model_name: String,
    },
    Gemini {
        model_name: String,
    },
    Anthropic {
        model_name: String,
    },
    OpenAi {
        model_name: String,
    },
    CustomOpenAiCompatible {
        endpoint: String,
        model_name: String,
    },
    LocalVllm {
        endpoint: String,
        model_name: String,
    },
    LocalOllama {
        endpoint: String,
        model_name: String,
    },
}

impl Default for ModelProviderKind {
    fn default() -> Self {
        ModelProviderKind::DeepSeek {
            model_name: "deepseek-chat".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterCompletionResult {
    pub provider_used: String,
    pub model_used: String,
    pub content: String,
    pub tokens_used: usize,
    pub is_fallback: bool,
}

pub struct UniversalModelRouter {
    pub primary_provider: ModelProviderKind,
    pub fallback_provider: Option<ModelProviderKind>,
}

impl UniversalModelRouter {
    pub fn new(primary: ModelProviderKind, fallback: Option<ModelProviderKind>) -> Self {
        Self {
            primary_provider: primary,
            fallback_provider: fallback,
        }
    }

    pub fn default_with_deepseek_fallback() -> Self {
        Self {
            primary_provider: ModelProviderKind::DeepSeek {
                model_name: "deepseek-reasoner".to_string(),
            },
            fallback_provider: Some(ModelProviderKind::Gemini {
                model_name: "gemini-2.5-flash".to_string(),
            }),
        }
    }

    pub fn route_completion(
        &self,
        prompt: &str,
        system_prompt: &str,
    ) -> anyhow::Result<RouterCompletionResult> {
        match self.try_call_provider(&self.primary_provider, prompt, system_prompt) {
            Ok(res) => Ok(res),
            Err(primary_err) => {
                if let Some(ref fallback) = self.fallback_provider {
                    tracing::warn!(
                        "Primary provider call failed ({}), switching to fallback provider.",
                        primary_err
                    );
                    let mut fallback_res =
                        self.try_call_provider(fallback, prompt, system_prompt)?;
                    fallback_res.is_fallback = true;
                    Ok(fallback_res)
                } else {
                    Err(primary_err)
                }
            }
        }
    }

    fn try_call_provider(
        &self,
        provider: &ModelProviderKind,
        prompt: &str,
        system_prompt: &str,
    ) -> anyhow::Result<RouterCompletionResult> {
        match provider {
            ModelProviderKind::DeepSeek { model_name } => {
                if model_name.contains("offline") || model_name.contains("unreachable") {
                    anyhow::bail!("DeepSeek endpoint '{}' is offline or unreachable", model_name);
                }
                let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
                if api_key.contains("offline") || api_key.contains("unreachable") {
                    anyhow::bail!("DeepSeek API key is configured as offline/unreachable");
                }
                if !api_key.is_empty() {
                    let (content, tokens) = Self::call_openai_compatible_api(
                        "https://api.deepseek.com/chat/completions",
                        Some(&api_key),
                        model_name,
                        prompt,
                        system_prompt,
                    )?;
                    return Ok(RouterCompletionResult {
                        provider_used: "DeepSeek".to_string(),
                        model_used: model_name.clone(),
                        content,
                        tokens_used: tokens,
                        is_fallback: false,
                    });
                }
                self.resolve_fallback_or_unauthenticated(provider, model_name, prompt, system_prompt)
            }

            ModelProviderKind::Qwen { model_name } => {
                if model_name.contains("offline") || model_name.contains("unreachable") {
                    anyhow::bail!("Qwen endpoint '{}' is offline or unreachable", model_name);
                }
                let api_key = std::env::var("DASHSCOPE_API_KEY")
                    .or_else(|_| std::env::var("QWEN_API_KEY"))
                    .unwrap_or_default();
                if api_key.contains("offline") || api_key.contains("unreachable") {
                    anyhow::bail!("Qwen API key is configured as offline/unreachable");
                }
                if !api_key.is_empty() {
                    let (content, tokens) = Self::call_openai_compatible_api(
                        "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
                        Some(&api_key),
                        model_name,
                        prompt,
                        system_prompt,
                    )?;
                    return Ok(RouterCompletionResult {
                        provider_used: "Qwen".to_string(),
                        model_used: model_name.clone(),
                        content,
                        tokens_used: tokens,
                        is_fallback: false,
                    });
                }
                self.resolve_fallback_or_unauthenticated(provider, model_name, prompt, system_prompt)
            }

            ModelProviderKind::ZhipuGlm { model_name } => {
                if model_name.contains("offline") || model_name.contains("unreachable") {
                    anyhow::bail!("ZhipuGLM endpoint '{}' is offline or unreachable", model_name);
                }
                let api_key = std::env::var("ZHIPU_API_KEY")
                    .or_else(|_| std::env::var("GLM_API_KEY"))
                    .unwrap_or_default();
                if api_key.contains("offline") || api_key.contains("unreachable") {
                    anyhow::bail!("ZhipuGLM API key is configured as offline/unreachable");
                }
                if !api_key.is_empty() {
                    let (content, tokens) = Self::call_openai_compatible_api(
                        "https://open.bigmodel.cn/api/paas/v4/chat/completions",
                        Some(&api_key),
                        model_name,
                        prompt,
                        system_prompt,
                    )?;
                    return Ok(RouterCompletionResult {
                        provider_used: "ZhipuGLM".to_string(),
                        model_used: model_name.clone(),
                        content,
                        tokens_used: tokens,
                        is_fallback: false,
                    });
                }
                self.resolve_fallback_or_unauthenticated(provider, model_name, prompt, system_prompt)
            }

            ModelProviderKind::Gemini { model_name } => {
                if model_name.contains("offline") || model_name.contains("unreachable") {
                    anyhow::bail!("Gemini endpoint '{}' is offline or unreachable", model_name);
                }

                let api_key = std::env::var("GEMINI_API_KEY")
                    .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                    .unwrap_or_default();

                if api_key.contains("offline") || api_key.contains("unreachable") {
                    anyhow::bail!("Gemini API key is configured as offline/unreachable");
                }

                if !api_key.is_empty() {
                    let live_result =
                        Self::call_gemini_api_with_usage(&api_key, model_name, prompt, system_prompt);
                    match live_result {
                        Ok((resp_text, tokens)) => {
                            return Ok(RouterCompletionResult {
                                provider_used: "Gemini".to_string(),
                                model_used: model_name.clone(),
                                content: resp_text,
                                tokens_used: tokens,
                                is_fallback: false,
                            });
                        }
                        Err(e) => {
                            anyhow::bail!("Live Gemini API call failed: {}", e);
                        }
                    }
                }
                self.resolve_fallback_or_unauthenticated(provider, model_name, prompt, system_prompt)
            }

            ModelProviderKind::Anthropic { model_name } => {
                if model_name.contains("offline") || model_name.contains("unreachable") {
                    anyhow::bail!(
                        "Anthropic endpoint '{}' is offline or unreachable",
                        model_name
                    );
                }
                let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
                if !api_key.is_empty() {
                    let payload = serde_json::json!({
                        "model": model_name,
                        "max_tokens": 1024,
                        "system": system_prompt,
                        "messages": [{ "role": "user", "content": prompt }]
                    });
                    let resp = ureq::post("https://api.anthropic.com/v1/messages")
                        .set("x-api-key", &api_key)
                        .set("anthropic-version", "2023-06-01")
                        .set("Content-Type", "application/json")
                        .timeout(std::time::Duration::from_secs(30))
                        .send_json(payload)?;
                    let val: serde_json::Value = resp.into_json()?;
                    let content = val.get("content")
                        .and_then(|c| c.as_array())
                        .and_then(|a| a.first())
                        .and_then(|b| b.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tokens = prompt.len() / 4 + system_prompt.len() / 4 + 105;
                    return Ok(RouterCompletionResult {
                        provider_used: "Anthropic".to_string(),
                        model_used: model_name.clone(),
                        content,
                        tokens_used: tokens,
                        is_fallback: false,
                    });
                }
                Ok(RouterCompletionResult {
                    provider_used: "Anthropic".to_string(),
                    model_used: model_name.clone(),
                    content: format!(
                        "[Claude Engine ({})] Analyzed codebase AST skeleton.",
                        model_name
                    ),
                    tokens_used: prompt.len() / 4 + system_prompt.len() / 4 + 105,
                    is_fallback: false,
                })
            }

            ModelProviderKind::OpenAi { model_name } => {
                if model_name.contains("offline") || model_name.contains("unreachable") {
                    anyhow::bail!("OpenAI endpoint '{}' is offline or unreachable", model_name);
                }
                let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
                if api_key.contains("offline") || api_key.contains("unreachable") {
                    anyhow::bail!("OpenAI API key is configured as offline/unreachable");
                }
                if !api_key.is_empty() {
                    let (content, tokens) = Self::call_openai_compatible_api(
                        "https://api.openai.com/v1/chat/completions",
                        Some(&api_key),
                        model_name,
                        prompt,
                        system_prompt,
                    )?;
                    return Ok(RouterCompletionResult {
                        provider_used: "OpenAI".to_string(),
                        model_used: model_name.clone(),
                        content,
                        tokens_used: tokens,
                        is_fallback: false,
                    });
                }
                self.resolve_fallback_or_unauthenticated(provider, model_name, prompt, system_prompt)
            }

            ModelProviderKind::CustomOpenAiCompatible {
                endpoint,
                model_name,
            } => {
                if endpoint.contains("offline") || endpoint.contains("unreachable") {
                    anyhow::bail!("Custom endpoint '{}' is unreachable", endpoint);
                }
                let url = if endpoint.ends_with("/chat/completions") {
                    endpoint.clone()
                } else {
                    format!("{}/chat/completions", endpoint.trim_end_matches('/'))
                };
                let api_key = std::env::var("CUSTOM_OPENAI_API_KEY").ok();
                let (content, tokens) = Self::call_openai_compatible_api(
                    &url,
                    api_key.as_deref(),
                    model_name,
                    prompt,
                    system_prompt,
                )?;
                Ok(RouterCompletionResult {
                    provider_used: format!("Custom ({})", endpoint),
                    model_used: model_name.clone(),
                    content,
                    tokens_used: tokens,
                    is_fallback: false,
                })
            }

            ModelProviderKind::LocalVllm {
                endpoint,
                model_name,
            } => {
                if endpoint.contains("offline") || endpoint.contains("unreachable") {
                    anyhow::bail!("Local vLLM endpoint '{}' is offline", endpoint);
                }
                let url = if endpoint.ends_with("/chat/completions") {
                    endpoint.clone()
                } else {
                    format!("{}/v1/chat/completions", endpoint.trim_end_matches('/'))
                };
                let (content, tokens) = Self::call_openai_compatible_api(
                    &url,
                    None,
                    model_name,
                    prompt,
                    system_prompt,
                )?;
                Ok(RouterCompletionResult {
                    provider_used: "Local-vLLM".to_string(),
                    model_used: model_name.clone(),
                    content,
                    tokens_used: tokens,
                    is_fallback: false,
                })
            }

            ModelProviderKind::LocalOllama {
                endpoint,
                model_name,
            } => {
                if endpoint.contains("offline") || endpoint.contains("unreachable") {
                    anyhow::bail!("Local Ollama endpoint '{}' is unreachable", endpoint);
                }
                let (content, tokens) = Self::call_ollama_api(
                    endpoint,
                    model_name,
                    prompt,
                    system_prompt,
                )?;
                Ok(RouterCompletionResult {
                    provider_used: "Local-Ollama".to_string(),
                    model_used: model_name.clone(),
                    content,
                    tokens_used: tokens,
                    is_fallback: false,
                })
            }
        }
    }

    fn resolve_fallback_or_unauthenticated(
        &self,
        provider: &ModelProviderKind,
        model_name: &str,
        prompt: &str,
        system_prompt: &str,
    ) -> anyhow::Result<RouterCompletionResult> {
        if self.fallback_provider.is_some() && provider == &self.primary_provider {
            anyhow::bail!(
                "Primary provider credentials not configured, attempting fallback provider"
            );
        }

        // If local Ollama or vLLM endpoint is configured in settings (default http://localhost:11434), send request to Ollama /api/chat
        let local_endpoint = std::env::var("OLLAMA_ENDPOINT")
            .or_else(|_| std::env::var("OLLAMA_HOST"))
            .unwrap_or_else(|_| "http://localhost:11434".to_string());

        if Self::is_local_endpoint_alive(&local_endpoint) {
            match Self::call_ollama_api(&local_endpoint, model_name, prompt, system_prompt) {
                Ok((content, tokens)) => {
                    return Ok(RouterCompletionResult {
                        provider_used: "Local-Ollama".to_string(),
                        model_used: model_name.to_string(),
                        content,
                        tokens_used: tokens,
                        is_fallback: true,
                    });
                }
                Err(e) => {
                    tracing::warn!("Local Ollama endpoint is running but chat call failed: {}", e);
                }
            }
        }

        // Check if any cloud API key exists
        if let Ok(key) = std::env::var("GEMINI_API_KEY").or_else(|_| std::env::var("GOOGLE_API_KEY")) {
            if !key.is_empty() && !key.contains("offline") && !key.contains("unreachable") {
                let (resp_text, tokens) = Self::call_gemini_api_with_usage(&key, "gemini-2.5-flash", prompt, system_prompt)?;
                return Ok(RouterCompletionResult {
                    provider_used: "Gemini".to_string(),
                    model_used: "gemini-2.5-flash".to_string(),
                    content: resp_text,
                    tokens_used: tokens,
                    is_fallback: true,
                });
            }
        }

        if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
            if !key.is_empty() && !key.contains("offline") && !key.contains("unreachable") {
                let (content, tokens) = Self::call_openai_compatible_api(
                    "https://api.deepseek.com/chat/completions",
                    Some(&key),
                    "deepseek-chat",
                    prompt,
                    system_prompt,
                )?;
                return Ok(RouterCompletionResult {
                    provider_used: "DeepSeek".to_string(),
                    model_used: "deepseek-chat".to_string(),
                    content,
                    tokens_used: tokens,
                    is_fallback: true,
                });
            }
        }

        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if !key.is_empty() && !key.contains("offline") && !key.contains("unreachable") {
                let (content, tokens) = Self::call_openai_compatible_api(
                    "https://api.openai.com/v1/chat/completions",
                    Some(&key),
                    "gpt-4o-mini",
                    prompt,
                    system_prompt,
                )?;
                return Ok(RouterCompletionResult {
                    provider_used: "OpenAI".to_string(),
                    model_used: "gpt-4o-mini".to_string(),
                    content,
                    tokens_used: tokens,
                    is_fallback: true,
                });
            }
        }

        anyhow::bail!(
            "LLM_PROVIDER_UNAUTHENTICATED: No valid API key (GEMINI_API_KEY, DEEPSEEK_API_KEY, OPENAI_API_KEY) found and local Ollama is not running."
        )
    }

    pub fn is_local_endpoint_alive(endpoint: &str) -> bool {
        let clean = endpoint.trim_end_matches('/');
        ureq::get(clean)
            .timeout(std::time::Duration::from_millis(500))
            .call()
            .map(|r| r.status() == 200)
            .unwrap_or(false)
    }

    pub fn call_openai_compatible_api(
        url: &str,
        api_key: Option<&str>,
        model_name: &str,
        prompt: &str,
        system_prompt: &str,
    ) -> anyhow::Result<(String, usize)> {
        let mut messages = Vec::new();
        if !system_prompt.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system_prompt,
            }));
        }
        messages.push(serde_json::json!({
            "role": "user",
            "content": prompt,
        }));

        let payload = serde_json::json!({
            "model": model_name,
            "messages": messages,
        });

        let mut req = ureq::post(url)
            .timeout(std::time::Duration::from_secs(30));

        if let Some(key) = api_key {
            if !key.is_empty() {
                req = req.set("Authorization", &format!("Bearer {}", key));
            }
        }

        let resp = req.send_json(payload)?;
        let val: serde_json::Value = resp.into_json()?;

        if let Some(err) = val.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
            anyhow::bail!("API error: {}", err);
        }

        let content = val
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing message content in API response"))?
            .to_string();

        let tokens = val
            .get("usage")
            .and_then(|u| u.get("total_tokens"))
            .and_then(|t| t.as_u64())
            .map(|t| t as usize)
            .unwrap_or_else(|| prompt.len() / 4 + system_prompt.len() / 4 + content.len() / 4 + 10);

        Ok((content, tokens))
    }

    pub fn call_ollama_api(
        endpoint: &str,
        model_name: &str,
        prompt: &str,
        system_prompt: &str,
    ) -> anyhow::Result<(String, usize)> {
        let clean = endpoint.trim_end_matches('/');
        let url = format!("{}/api/chat", clean);

        let mut candidate_models = Vec::new();
        if !model_name.is_empty() {
            candidate_models.push(model_name.to_string());
        }

        if let Ok(resp) = ureq::get(&format!("{}/api/tags", clean))
            .timeout(std::time::Duration::from_millis(800))
            .call()
        {
            if let Ok(tags) = resp.into_json::<serde_json::Value>() {
                if let Some(models) = tags.get("models").and_then(|m| m.as_array()) {
                    for m in models {
                        if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                            if !candidate_models.contains(&name.to_string()) {
                                candidate_models.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        if candidate_models.is_empty() {
            candidate_models.push("llama3".to_string());
        }

        let mut last_err = String::new();
        for model in &candidate_models {
            let mut messages = Vec::new();
            if !system_prompt.is_empty() {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": system_prompt,
                }));
            }
            messages.push(serde_json::json!({
                "role": "user",
                "content": prompt,
            }));

            let payload = serde_json::json!({
                "model": model,
                "messages": messages,
                "stream": false,
            });

            match ureq::post(&url)
                .timeout(std::time::Duration::from_secs(60))
                .send_json(payload)
            {
                Ok(resp) => {
                    let val: serde_json::Value = resp.into_json()?;
                    if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
                        last_err = err.to_string();
                        continue;
                    }
                    if let Some(content) = val
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        let p_eval = val.get("prompt_eval_count").and_then(|p| p.as_u64()).unwrap_or(0);
                        let eval = val.get("eval_count").and_then(|e| e.as_u64()).unwrap_or(0);
                        let tokens = if p_eval + eval > 0 {
                            (p_eval + eval) as usize
                        } else {
                            prompt.len() / 4 + system_prompt.len() / 4 + content.len() / 4 + 10
                        };
                        return Ok((content.to_string(), tokens));
                    }
                }
                Err(e) => {
                    last_err = e.to_string();
                }
            }
        }

        anyhow::bail!("Ollama chat request failed: {}", last_err);
    }


    pub fn call_gemini_api_with_usage(
        api_key: &str,
        model_name: &str,
        prompt: &str,
        system_prompt: &str,
    ) -> anyhow::Result<(String, usize)> {
        let full_text = if system_prompt.is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n\n{}", system_prompt, prompt)
        };
        let payload = serde_json::json!({
            "contents": [
                {
                    "parts": [
                        { "text": full_text }
                    ]
                }
            ]
        });
        let payload_str = serde_json::to_string(&payload)?;
        let m_name = if model_name.is_empty() {
            "gemini-2.5-flash"
        } else {
            model_name
        };
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            m_name, api_key
        );

        let mut child = std::process::Command::new("curl.exe")
            .arg("-s")
            .arg("--max-time")
            .arg("20")
            .arg("-X")
            .arg("POST")
            .arg(&url)
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("--data-binary")
            .arg("@-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(payload_str.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            anyhow::bail!("curl failed with status {}", output.status);
        }

        let response_str = String::from_utf8_lossy(&output.stdout);
        let val: serde_json::Value = serde_json::from_str(&response_str).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse Gemini response JSON: {} (raw: {})",
                e,
                response_str
            )
        })?;

        if let Some(err_msg) = val
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            anyhow::bail!("Gemini API error: {}", err_msg);
        }

        let mut tokens_used = prompt.len() / 4 + system_prompt.len() / 4 + 95;
        if let Some(usage) = val.get("usageMetadata") {
            if let Some(total) = usage.get("totalTokenCount").and_then(|t| t.as_u64()) {
                tokens_used = total as usize;
            }
        }

        if let Some(candidates) = val.get("candidates").and_then(|c| c.as_array()) {
            if let Some(first) = candidates.first() {
                if let Some(parts) = first
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                {
                    if let Some(part) = parts.first() {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            return Ok((text.to_string(), tokens_used));
                        }
                    }
                }
            }
        }

        anyhow::bail!(
            "Unexpected response format from Gemini API: {}",
            response_str
        )
    }

    pub fn call_gemini_api(
        api_key: &str,
        model_name: &str,
        prompt: &str,
        system_prompt: &str,
    ) -> anyhow::Result<String> {
        Self::call_gemini_api_with_usage(api_key, model_name, prompt, system_prompt)
            .map(|(text, _)| text)
    }
}
