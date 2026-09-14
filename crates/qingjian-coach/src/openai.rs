use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage, CreateChatCompletionRequestArgs, ReasoningEffort,
    ResponseFormat,
};

use crate::config::CoachConfig;
use crate::error::CoachError;
use crate::prompt;
use crate::provider::{CoachProvider, EnglishCoachRequest, EnglishCoachResponse};

/// Coach 回复的 token 上限：一句英文加两条短语足够。
const MAX_TOKENS: u32 = 300;

/// 采样温度：表达要自然，也不要花。
const TEMPERATURE: f32 = 0.4;

/// OpenAI 兼容接口的 English Coach 供应商：一个请求进、一句地道英文加几条短语出。
pub struct OpenAICoach {
    client: Client<OpenAIConfig>,

    /// 模型名。
    model: String,

    /// 超时。
    timeout: Duration,

    /// 推理强度；`None` 表示不发这个参数。
    reasoning_effort: Option<ReasoningEffort>,
}

impl OpenAICoach {
    /// 没有密钥直接报错，让壳退回不启用并记日志。
    pub fn new(config: &CoachConfig) -> Result<Self, CoachError> {
        let api_key = config
            .resolve_api_key()
            .ok_or_else(|| CoachError::MissingApiKey(config.api_key_env.clone()))?;
        let openai = OpenAIConfig::new()
            .with_api_base(config.base_url.trim_end_matches('/'))
            .with_api_key(api_key);
        Ok(Self {
            client: Client::with_config(openai),
            model: config.model.clone(),
            timeout: Duration::from_millis(config.timeout_ms),
            reasoning_effort: parse_reasoning_effort(&config.reasoning_effort),
        })
    }

    /// 一问一答：系统提示 + 用户消息，要 JSON 对象，返回正文。
    async fn chat(&self, request: &EnglishCoachRequest) -> Result<String, CoachError> {
        let user = prompt::user_prompt(request);
        let messages: Vec<ChatCompletionRequestMessage> = vec![
            ChatCompletionRequestSystemMessage::from(prompt::system_prompt()).into(),
            ChatCompletionRequestUserMessage::from(user).into(),
        ];
        let mut args = CreateChatCompletionRequestArgs::default();
        args.model(&self.model)
            .messages(messages)
            .max_tokens(MAX_TOKENS)
            .temperature(TEMPERATURE)
            .response_format(ResponseFormat::JsonObject);
        if let Some(effort) = self.reasoning_effort.clone() {
            args.reasoning_effort(effort);
        }
        let body = args.build()?;
        let response = tokio::time::timeout(self.timeout, self.client.chat().create(body))
            .await
            .map_err(|_| CoachError::Timeout(self.timeout.as_millis() as u64))??;
        let content = response
            .choices
            .into_iter()
            .find_map(|choice| choice.message.content.filter(|c| !c.trim().is_empty()))
            .ok_or(CoachError::EmptyReply)?;
        tracing::debug!(%content, "English Coach 回复");
        Ok(content)
    }
}

impl CoachProvider for OpenAICoach {
    fn generate(
        &self,
        request: EnglishCoachRequest,
    ) -> Pin<Box<dyn Future<Output = Result<EnglishCoachResponse, CoachError>> + Send + '_>> {
        Box::pin(async move {
            let content = self.chat(&request).await?;
            prompt::parse_reply(&content, request.max_phrases).map_err(CoachError::Format)
        })
    }
}

/// 配置里的推理强度字符串转成接口枚举；留空不发，认不得的值当留空并记一条警告。
fn parse_reasoning_effort(value: &str) -> Option<ReasoningEffort> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "none" => Some(ReasoningEffort::None),
        "minimal" => Some(ReasoningEffort::Minimal),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" => Some(ReasoningEffort::Xhigh),
        other => {
            tracing::warn!(value = other, "reasoning_effort 不认识，不发这个参数");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_effort_parses_known_values_and_ignores_the_rest() {
        assert!(matches!(
            parse_reasoning_effort(" none "),
            Some(ReasoningEffort::None)
        ));
        assert!(parse_reasoning_effort("maximum").is_none());
    }
}
