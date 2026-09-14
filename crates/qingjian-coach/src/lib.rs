//! English Coach 的核心：句子上下文、AI Provider 抽象与后台请求线程。
//!
//! 平台层只做三件事：把上屏的中文喂给 [`SentenceContext`]，把 [`CoachRequest`]
//! 交给 [`CoachService`]，拿 [`CoachOutcome` 前的结果去画界面。AI 永远异步，
//! 中文输入链路完全不经过这里。

mod config;
mod error;
mod openai;
mod prompt;
mod provider;
mod sentence;
mod service;
mod worker;

pub use config::CoachConfig;
pub use error::CoachError;
pub use provider::{CoachProvider, EnglishCoachRequest, EnglishCoachResponse, LearningPhrase};
pub use sentence::{Feed, SentenceContext};
pub use service::CoachService;
