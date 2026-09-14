use thiserror::Error;

/// English Coach 的错误。**只**影响英文面板的显示，永远不影响中文输入。
#[derive(Debug, Error)]
pub enum CoachError {
    /// 没有密钥：配置里没填，环境变量也没有。
    #[error("没有 API 密钥。在上面填一个，或设置环境变量 {0}")]
    MissingApiKey(String),

    /// 请求超时。
    #[error("{0} ms 内没有回复")]
    Timeout(u64),

    /// 模型回了，但没有正文。
    #[error("接口通了但没有返回正文")]
    EmptyReply,

    /// 起不了后台线程。
    #[error("起不了后台线程：{0}")]
    Runtime(#[from] std::io::Error),

    /// 接口返回解析不了。
    #[error("返回格式不对：{0}")]
    Format(String),

    /// 接口调用失败（网络、鉴权、限流……）。
    #[error("请求失败：{0}")]
    Api(String),
}

impl From<async_openai::error::OpenAIError> for CoachError {
    fn from(error: async_openai::error::OpenAIError) -> Self {
        Self::Api(error.to_string())
    }
}
