//! 后台线程：收请求、防抖、发网络请求、回结果。

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::error::CoachError;
use crate::openai::OpenAICoach;
use crate::provider::CoachProvider;
use crate::provider::{EnglishCoachRequest, EnglishCoachResponse};

/// 发给后台线程的请求：就是 `EnglishCoachRequest`，句子与版本号由壳在发出时定好。
pub type CoachTask = EnglishCoachRequest;

/// 一次请求的结局：成功带回复，失败带错误，句子与版本号原样带回供壳校验。
#[derive(Debug)]
pub struct CoachOutcome {
    pub sentence_id: u64,
    pub version: u64,
    pub result: Result<EnglishCoachResponse, CoachError>,
}

pub struct Worker {
    /// 请求入口。主线程 drop 掉发送端后线程自然退出。
    requests: Receiver<CoachTask>,

    /// 结果出口。
    responses: Sender<CoachOutcome>,

    /// 网络客户端。
    provider: OpenAICoach,

    /// 防抖窗口。
    debounce: Duration,
}

impl Worker {
    pub fn new(
        requests: Receiver<CoachTask>,
        responses: Sender<CoachOutcome>,
        provider: OpenAICoach,
        debounce: Duration,
    ) -> Self {
        Self {
            requests,
            responses,
            provider,
            debounce,
        }
    }

    /// 阻塞运行直到发送端全部关闭。
    pub fn run(mut self) -> Result<(), CoachError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        while let Some(request) = self.next_task()? {
            let start = Instant::now();
            let result = runtime.block_on(self.provider.generate(request.clone()));
            match &result {
                Ok(response) => tracing::info!(
                    sentence_id = request.sentence_id,
                    version = request.version,
                    elapsed_ms = start.elapsed().as_millis(),
                    phrases = response.phrases.len(),
                    "English Coach 完成"
                ),
                // 错误只记日志：中文输入无感，面板保留上一次结果
                Err(error) => tracing::warn!(
                    sentence_id = request.sentence_id,
                    version = request.version,
                    %error,
                    "English Coach 失败"
                ),
            }
            self.reply(CoachOutcome {
                sentence_id: request.sentence_id,
                version: request.version,
                result,
            });
        }
        Ok(())
    }

    /// 取下一个要真正发出去的请求：完结的（强触发）不等防抖立刻发；
    /// 未完结的（弱触发）在防抖窗口内持续有新的就只留最后一个。
    /// 发送端全部关闭返回 `None`。
    fn next_task(&mut self) -> Result<Option<CoachTask>, CoachError> {
        // Err 只会是发送端全部关闭
        let Ok(mut latest) = self.requests.recv() else {
            return Ok(None);
        };
        loop {
            if latest.completed {
                return Ok(Some(latest));
            }
            match self.requests.recv_timeout(self.debounce) {
                Ok(newer) => latest = newer,
                Err(RecvTimeoutError::Timeout) => return Ok(Some(latest)),
                // 窗口里断了：手里这个还是要发出去，下一轮再退
                Err(RecvTimeoutError::Disconnected) => return Ok(Some(latest)),
            }
        }
    }

    fn reply(&self, outcome: CoachOutcome) {
        // 接收端没了说明 CoachService 已经被 drop，线程随后也会退出
        let _ = self.responses.send(outcome);
    }
}
