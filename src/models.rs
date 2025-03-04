use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ModelProvider {
    pub name: String,
    pub api_base: String,
    pub api_key_env: String,
    pub models: Vec<Model>,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatResponseChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponseChoice {
    pub delta: ChatResponseDelta,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponseDelta {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
}

pub fn get_providers() -> Vec<ModelProvider> {
    vec![
        ModelProvider {
            name: String::from("deepseek"),
            api_base: String::from("https://ark.cn-beijing.volces.com/api/v3/chat/completions"),
            api_key_env: String::from("OPENAI_API_KEY"),
            models: vec![
                Model {
                    id: String::from("deepseek-r1-250120"),
                    name: String::from("deepseek-r1"),
                    provider: String::from("deepseek"),
                },
                Model {
                    id: String::from("deepseek-v3-241226"),
                    name: String::from("deepseek-v3"),
                    provider: String::from("deepseek"),
                },
            ],
        },
        ModelProvider {
            name: String::from("openrouter"),
            api_base: String::from("https://openrouter.ai/api/v1/chat/completions"),
            api_key_env: String::from("OPENROUTER_API_KEY"),
            models: vec![
                Model {
                    id: String::from("google/gemini-2.0-flash-001"),
                    name: String::from("gemini-flash"),
                    provider: String::from("openrouter"),
                },
                Model {
                    id: String::from("google/gemini-2.0-flash-lite-001"),
                    name: String::from("gemini-flash-lite"),
                    provider: String::from("openrouter"),
                },
                Model {
                    id: String::from("google/gemini-2.0-pro-exp-02-05"),
                    name: String::from("gemini-pro"),
                    provider: String::from("openrouter"),
                },
            ],
        },
    ]
}

pub fn find_models(query: &str) -> Vec<Model> {
    let providers = get_providers();
    let mut matches = Vec::new();
    
    for provider in providers {
        for model in provider.models {
            if model.name.contains(query) || model.id.contains(query) {
                matches.push(model);
            }
        }
    }
    
    matches
}

pub fn get_provider_by_model(model_id: &str) -> Option<ModelProvider> {
    get_providers().into_iter().find(|p| p.models.iter().any(|m| m.id == model_id))
}