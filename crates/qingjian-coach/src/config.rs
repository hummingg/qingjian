use serde::{Deserialize, Serialize};

/// English Coach 配置。默认**关闭**；开启后上屏的中文句子会发往 `base_url` 换取地道英文表达。
///
/// 中文输入永远不等这里的任何结果：超时、断网、密钥错误都只影响英文面板，不影响输入。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CoachConfig {
    /// 是否启用。
    pub enabled: bool,

    /// OpenAI 兼容接口地址（不含 `/chat/completions`）。
    pub base_url: String,

    /// 模型名。
    pub model: String,

    /// 密钥。留空则读 `api_key_env` 指定的环境变量。
    pub api_key: Option<String>,

    /// 存放密钥的环境变量名。
    pub api_key_env: String,

    /// 单次请求超时（毫秒），超时即丢。
    pub timeout_ms: u64,

    /// 防抖：上屏文本停止变化多久之后才真正发请求（毫秒）。弱触发的停顿阈值。
    pub debounce_ms: u64,

    /// 给模型看的前一句最多多少个字符（当前句永远全发）。
    pub previous_chars: usize,

    /// 用户英语水平（A1–C2），控制英文表达的难度；留空按 B1 处理。
    pub level: String,

    /// 一句话最多返回几条学习短语。
    pub max_phrases: usize,

    /// 推理强度，随请求发 `reasoning_effort`；留空则不发（给不认这个参数的接口）。
    pub reasoning_effort: String,
}

impl Default for CoachConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://api.deepseek.com".to_owned(),
            model: "deepseek-v4-flash".to_owned(),
            api_key: None,
            api_key_env: "QINGJIAN_COACH_API_KEY".to_owned(),
            timeout_ms: 8000,
            debounce_ms: 800,
            previous_chars: 64,
            level: String::new(),
            max_phrases: 2,
            reasoning_effort: "none".to_owned(),
        }
    }
}

impl CoachConfig {
    /// 配置里的密钥优先，其次环境变量；两边都没有返回 `None`。
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .or_else(|| std::env::var(&self.api_key_env).ok())
            .filter(|k| !k.trim().is_empty())
    }

    /// 请求里带的用户水平；留空按 B1。
    pub fn learner_level(&self) -> &str {
        match self.level.trim() {
            "" => "B1",
            level => level,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_level_falls_back_to_b1() {
        assert_eq!(CoachConfig::default().learner_level(), "B1");
        let advanced = CoachConfig {
            level: " C2 ".to_owned(),
            ..CoachConfig::default()
        };
        assert_eq!(advanced.learner_level(), "C2");
    }

    #[test]
    fn api_key_prefers_config_over_environment() {
        let blank = CoachConfig {
            api_key: Some("  ".to_owned()),
            ..CoachConfig::default()
        };
        assert_eq!(blank.resolve_api_key(), None);
        let from_config = CoachConfig {
            api_key: Some("from-config".to_owned()),
            ..CoachConfig::default()
        };
        assert_eq!(
            from_config.resolve_api_key().as_deref(),
            Some("from-config")
        );
    }
}
