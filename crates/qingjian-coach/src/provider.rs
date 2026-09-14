use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::CoachError;

/// 值得学的一条表达：短语、中文释义、难度与例句。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningPhrase {
    /// 英文短语，如 `get back to somebody`。
    pub phrase: String,

    /// 中文释义，如 `回复某人 / 回头联系某人`。
    pub meaning: String,

    /// CEFR 等级（A1–C2），模型给不出就留空。
    #[serde(default)]
    pub cefr: String,

    /// 一句例句，可选。
    #[serde(default)]
    pub example: String,
}

/// 一次 English Coach 请求：当前句子 + 受控的前文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnglishCoachRequest {
    /// 当前中文句子（强触发完结的，或用户停顿时的半句）。
    pub text: String,

    /// 上一句的结尾，帮模型接住指代与省略；超过配置长度的会被截短。
    pub previous: Option<String>,

    /// 用户英语水平（A1–C2）。
    pub learner_level: String,

    /// 一句话最多返回几条学习短语。
    pub max_phrases: usize,

    /// 请求发起时的句子与版本号：结果原样带回，壳用来防旧覆新。
    pub sentence_id: u64,
    pub version: u64,

    /// 句子是否已完结（强触发）。完结的请求后台线程立即发送，不再等防抖窗口。
    pub completed: bool,
}

/// 一次 English Coach 回复。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnglishCoachResponse {
    /// 地道英文表达（整句）。
    pub english: String,

    /// 值得学习的短语，最多 `max_phrases` 条。
    pub phrases: Vec<LearningPhrase>,
}

/// AI 供应商的抽象：第一阶段只有 OpenAI 兼容实现，接口保持稳定，
/// 以后接 Ollama、Gemini 或本地模型时平台层不用改。
///
/// 返回 boxed future 而不是 `async fn`，是为了让 trait 对象（`Box<dyn CoachProvider>`）可用。
pub trait CoachProvider: Send + Sync {
    fn generate(
        &self,
        request: EnglishCoachRequest,
    ) -> Pin<Box<dyn Future<Output = Result<EnglishCoachResponse, CoachError>> + Send + '_>>;
}
