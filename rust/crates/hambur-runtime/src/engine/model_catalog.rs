use crate::*;

const MODEL_CATALOG_CACHE_KEY: &str = "models_dev_api";
const MODEL_CATALOG_CACHE_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";

impl RuntimeEngine {
    pub(crate) fn schedule_model_catalog_sync(self: &Arc<Self>) {
        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return;
        };
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            sleep(Duration::from_secs(15)).await;
            if engine.shutdown.load(Ordering::SeqCst) {
                return;
            }
            let _ = engine.models_dev_catalog_json(false).await;
        });
    }

    pub(crate) async fn provider_models_response(
        &self,
        provider_id: &str,
        command: &RuntimeCommand,
        payload: &Value,
    ) -> HamburResult<String> {
        if payload
            .get("data")
            .and_then(Value::as_array)
            .is_some_and(|data| !data.is_empty())
        {
            return Ok(command.payload_json.clone());
        }

        let provider = self.database.provider_by_id(provider_id).ok();
        let base_url = command.chunk.trim().to_string().if_blank(
            provider
                .as_ref()
                .map(|value| value.base_url.clone())
                .unwrap_or_default(),
        );
        let api_key = payload
            .get("apiKey")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
            .if_blank(provider_api_key_from_payload(payload))
            .if_blank(
                provider
                    .as_ref()
                    .and_then(|value| secret_ref_to_api_key(&value.secret_ref))
                    .unwrap_or_default(),
            );
        let api_key = if api_key.trim().is_empty() {
            if let Some(provider) = provider.as_ref() {
                self.resolve_provider_api_key(
                    "",
                    "",
                    &ModelRouteSnapshot {
                        provider_id: provider.id.clone(),
                        provider_name: provider.name.clone(),
                        provider_protocol: provider.api_type.clone(),
                        base_url: provider.base_url.clone(),
                        secret_ref: provider.secret_ref.clone(),
                        model_id: command.model_id.clone(),
                        model_display_name: command.model_id.clone(),
                        ..ModelRouteSnapshot::default()
                    },
                    &Arc::new(AtomicBool::new(false)),
                )
                .await
                .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            api_key
        };

        if !base_url.trim().is_empty() {
            if let Ok(response) = fetch_openai_compatible_models(&base_url, &api_key).await
                && response.trim().starts_with('{')
            {
                return Ok(response);
            }
        }

        Ok(default_models_response(&command.model_id))
    }

    pub(crate) async fn models_dev_catalog_json(&self, force: bool) -> HamburResult<String> {
        let cached = self
            .database
            .model_catalog_cache(MODEL_CATALOG_CACHE_KEY)?;
        if !force
            && let Some(cache) = cached.as_ref()
            && cache.synced_at_ms > 0
            && now_ms().saturating_sub(cache.synced_at_ms) < MODEL_CATALOG_CACHE_MAX_AGE_MS
            && !cache.catalog_json.trim().is_empty()
        {
            return Ok(cache.catalog_json.clone());
        }

        let fetched = reqwest_text_url(MODELS_DEV_API_URL, 20).await?;
        let _: Value = serde_json::from_str(&fetched).map_err(|error| {
            HamburError::ProviderUnavailable(format!("parse models.dev catalog: {error}"))
        })?;
        self.database
            .upsert_model_catalog_cache(MODEL_CATALOG_CACHE_KEY, &fetched, now_ms())?;
        Ok(fetched)
    }

    pub(crate) async fn enrich_provider_models_from_catalog(&self, models: &mut [ProviderModel]) {
        let Ok(catalog_json) = self.models_dev_catalog_json(false).await else {
            return;
        };
        let Ok(catalog) = serde_json::from_str::<Value>(&catalog_json) else {
            return;
        };
        for model in models {
            let Some(detail) = match_catalog_model(&catalog, &model.model_id) else {
                continue;
            };
            merge_catalog_detail_into_provider_model(model, detail);
        }
    }
}

fn merge_catalog_detail_into_provider_model(model: &mut ProviderModel, detail: &Value) {
    if let Some(name) = detail
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        model.display_name = name.to_string();
    }
    model.capabilities.supports_tool_call =
        detail_bool(detail, "tool_call", model.capabilities.supports_tool_call);
    model.capabilities.supports_reasoning =
        detail_bool(detail, "reasoning", model.capabilities.supports_reasoning);
    model.capabilities.supports_image_input =
        detail_bool(
            detail,
            "attachment",
            model.capabilities.supports_image_input,
        ) || detail_modalities_include(detail, "input", "image")
            || detail_modalities_include(detail, "input", "vision");
    model.capabilities.supports_structured_output = detail_bool(
        detail,
        "structured_output",
        model.capabilities.supports_structured_output,
    );
    model.capabilities.supports_temperature = detail_bool(
        detail,
        "temperature",
        model.capabilities.supports_temperature,
    );
    if let Some(context_limit) = detail
        .get("limit")
        .and_then(|limit| limit.get("context").or_else(|| limit.get("input")))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        model.capabilities.context_limit = context_limit;
    }
    if let Some(output_limit) = detail
        .get("limit")
        .and_then(|limit| limit.get("output"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        model.capabilities.output_limit = output_limit;
    }
    if let Some(reasoning_field) = detail
        .get("interleaved")
        .and_then(|value| {
            value
                .get("field")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
        })
        .filter(|value| !value.trim().is_empty())
    {
        model.capabilities.reasoning_field = reasoning_field.to_string();
    }
    model.metadata_json = detail.to_string();
}

fn match_catalog_model<'a>(catalog: &'a Value, model_id: &str) -> Option<&'a Value> {
    let normalized = normalize_model_id(model_id);
    let providers = catalog
        .get("providers")
        .and_then(Value::as_object)
        .or_else(|| catalog.as_object());
    let global_models = catalog.get("models").and_then(Value::as_object);

    if let Some(models) = global_models {
        if let Some(detail) = models.get(model_id) {
            return Some(detail);
        }
        if let Some((_, detail)) = models
            .iter()
            .find(|(id, _)| normalize_model_id(id) == normalized)
        {
            return Some(detail);
        }
        if let Some((_, detail)) = models
            .iter()
            .find(|(id, _)| normalize_model_id(id).ends_with(&format!("/{normalized}")))
        {
            return Some(detail);
        }
    }

    let providers = providers?;
    for provider in providers.values() {
        let Some(models) = provider.get("models").and_then(Value::as_object) else {
            continue;
        };
        if let Some(detail) = models.get(model_id) {
            return Some(detail);
        }
        if let Some((_, detail)) = models.iter().find(|(id, detail)| {
            normalize_model_id(id) == normalized
                || detail
                    .get("id")
                    .and_then(Value::as_str)
                    .map(normalize_model_id)
                    .as_deref()
                    == Some(normalized.as_str())
                || detail
                    .get("name")
                    .and_then(Value::as_str)
                    .map(normalize_model_id)
                    .as_deref()
                    == Some(normalized.as_str())
        }) {
            return Some(detail);
        }
        if let Some((_, detail)) = models.iter().find(|(id, detail)| {
            normalize_model_id(id).ends_with(&format!("/{normalized}"))
                || detail
                    .get("id")
                    .and_then(Value::as_str)
                    .map(normalize_model_id)
                    .is_some_and(|value| value.ends_with(&format!("/{normalized}")))
        }) {
            return Some(detail);
        }
    }
    None
}

fn normalize_model_id(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn detail_bool(detail: &Value, key: &str, fallback: bool) -> bool {
    detail.get(key).and_then(Value::as_bool).unwrap_or(fallback)
}

fn detail_modalities_include(detail: &Value, direction: &str, expected: &str) -> bool {
    detail
        .get("modalities")
        .and_then(|modalities| modalities.get(direction))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .any(|value| value.eq_ignore_ascii_case(expected))
        })
        .unwrap_or(false)
}

async fn fetch_openai_compatible_models(base_url: &str, api_key: &str) -> HamburResult<String> {
    let url = openai_models_url(base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let mut request = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "Hambur/0.1");
    if !api_key.trim().is_empty() {
        request = request.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", api_key.trim()),
        );
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
        } else {
            HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_provider_http_status(status));
    }
    response
        .text()
        .await
        .map_err(|error| HamburError::ProviderUnavailable(format!("NetworkError: {error}")))
}

fn openai_models_url(base_url: &str) -> String {
    let base_url = base_url.trim();
    if base_url.ends_with("/models") || base_url.ends_with("/models/") {
        base_url.to_string()
    } else if base_url.ends_with('/') {
        format!("{base_url}models")
    } else {
        format!("{base_url}/models")
    }
}

fn provider_api_key_from_payload(payload: &Value) -> String {
    payload
        .get("secretRef")
        .and_then(Value::as_str)
        .and_then(secret_ref_to_api_key)
        .unwrap_or_default()
}

fn secret_ref_to_api_key(secret_ref: &str) -> Option<String> {
    let env_name = secret_ref.strip_prefix("env://")?;
    std::env::var(env_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
