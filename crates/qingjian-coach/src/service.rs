//! 平台壳接的入口：构造时起一个后台线程，`submit` / `poll` 都只碰通道，不阻塞。

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crate::config::CoachConfig;
use crate::error::CoachError;
use crate::openai::OpenAICoach;
use crate::provider::EnglishCoachRequest;
use crate::worker::{CoachOutcome, Worker};

/// 走网络的 English Coach 服务。中文输入链路不经过这里；接口任何失败都只是「没有新英文」。
pub struct CoachService {
    /// 往后台线程发请求。
    requests: Sender<EnglishCoachRequest>,

    /// 从后台线程收结果。
    responses: Receiver<CoachOutcome>,
}

impl CoachService {
    /// 没有密钥直接报错，让壳保持关闭并记日志。
    pub fn new(config: &CoachConfig) -> Result<Self, CoachError> {
        let provider = OpenAICoach::new(config)?;
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        let worker = Worker::new(
            request_rx,
            response_tx,
            provider,
            Duration::from_millis(config.debounce_ms),
        );
        std::thread::Builder::new()
            .name("qingjian-coach".to_owned())
            .spawn(move || {
                if let Err(error) = worker.run() {
                    tracing::error!(%error, "English Coach 线程退出");
                }
            })?;
        tracing::info!(
            base_url = %config.base_url,
            model = %config.model,
            level = config.learner_level(),
            debounce_ms = config.debounce_ms,
            "English Coach 已启用"
        );
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
        })
    }

    /// 发一次请求；线程已退出时静默丢弃（记日志）。
    pub fn submit(&self, request: EnglishCoachRequest) {
        if self.requests.send(request).is_err() {
            tracing::warn!("English Coach 线程已退出，请求被丢弃");
        }
    }

    /// 收一次结果；没有就返回 `None`。线程退出时同样返回 `None`（退出已经记过日志）。
    pub fn poll(&mut self) -> Option<CoachOutcome> {
        self.responses.try_recv().ok()
    }
}
