//! English Coach 在壳里的接线：上屏的中文喂进 [`SentenceContext`]，请求交给
//! [`CoachService`] 的后台线程，结果到期校验句子与版本号后画到面板上。
//!
//! 每一步都允许失败：服务没起、请求没发、结果过期、接口报错，全都只是
//! 「面板不更新」，中文输入链路不经过这里的任何一行。

use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{NSObject, NSObjectProtocol, NSTimer};

use super::*;
use crate::coach::CoachFrame;
use crate::imk::secure_input;

/// 轮询间隔。
const POLL_INTERVAL: f64 = 0.1;

/// 最长轮询多久；请求本身有超时，这里只是兜底（超时 + 防抖 + 余量）。
const MAX_WAIT: Duration = Duration::from_secs(30);

/// 给模型看的前一句太长时只留结尾：靠近当前句的字对接指代最有用。
fn tail_chars(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    text.chars().skip(count - max).collect()
}

/// English Coach 结果的轮询定时器：发出请求时开始轮询，结果到了就停，平时不占 CPU。
pub struct CoachMonitor {
    /// 定时器；没在等结果时为 `None`。
    timer: Option<Retained<NSTimer>>,

    /// 本轮开始等待的时间。
    since: Option<Instant>,

    mtm: MainThreadMarker,
}

impl CoachMonitor {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self {
            timer: None,
            since: None,
            mtm,
        }
    }

    /// 有请求在飞：开始（或继续）轮询。
    pub fn start(&mut self) {
        self.since = Some(Instant::now());
        if self.timer.is_some() {
            return;
        }
        let target = CoachTicker::new(self.mtm);
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                POLL_INTERVAL,
                &target,
                sel!(tick:),
                None,
                true,
            )
        };
        self.timer = Some(timer);
    }

    pub fn stop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.invalidate();
        }
        self.since = None;
    }

    /// 等太久了就放弃。
    pub fn expired(&self) -> bool {
        self.since.is_some_and(|since| since.elapsed() > MAX_WAIT)
    }
}

define_class!(
    // SAFETY: NSObject 没有子类化要求；没有实现 Drop。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct CoachTicker;

    impl CoachTicker {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: Option<&AnyObject>) {
            crate::host::with(|h| h.poll_coach());
        }
    }

    unsafe impl NSObjectProtocol for CoachTicker {}
);

impl CoachTicker {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

impl Host {
    /// 把当前 `[coach]` 套用进来：变了才重建服务（重建会起新线程），开关一关就收摊。
    pub(super) fn apply_coach(&mut self, coach: &CoachConfig) {
        if self.applied_coach == *coach {
            return;
        }
        self.applied_coach = coach.clone();
        self.coach_monitor.stop();
        self.coach_context = SentenceContext::default();
        self.coach_frame = None;
        self.coach_panel.hide();
        self.coach = if coach.enabled {
            match CoachService::new(coach) {
                Ok(service) => Some(service),
                Err(error) => {
                    tracing::warn!(%error, "English Coach 未启用");
                    None
                }
            }
        } else {
            None
        };
    }

    /// 一段中文上了屏：喂进句子上下文，变了就安排一次 AI 请求。
    /// Secure Input（密码框）里整段不看不发：句子上下文当场清空，隐私优先。
    pub fn coach_commit(&mut self, text: &str) {
        if self.coach.is_none() {
            return;
        }
        if secure_input::enabled() {
            tracing::debug!("Secure Input 中，English Coach 不看不发");
            self.coach_context = SentenceContext::default();
            return;
        }
        if self.coach_context.feed(text).changed {
            self.coach_submit();
        }
    }

    /// 句子被换行、切焦点这类事件打断：把手头的半句当完结发出去。
    pub fn coach_break(&mut self) {
        if self.coach.is_none() {
            return;
        }
        if self.coach_context.break_sentence().changed {
            self.coach_submit();
        }
    }

    /// 组句外的退格：句子上下文同步删一个字，删空了连面板一起收掉。
    pub fn coach_backspace(&mut self) {
        if self.coach.is_none() {
            return;
        }
        self.coach_context.backspace();
        if self.coach_context.is_empty() {
            self.coach_frame = None;
            self.coach_panel.hide();
        } else {
            self.coach_submit();
        }
    }

    /// 应用那边整段删了词 / 删到行首：句子上下文已经对不上，整句作废、面板收掉。
    pub fn coach_reset(&mut self) {
        if self.coach.is_none() {
            return;
        }
        self.coach_monitor.stop();
        self.coach_context.clear();
        self.coach_frame = None;
        self.coach_panel.hide();
    }

    /// 输入法被停用：与 [`Self::coach_reset`] 相同的收摊，服务留着下次用。
    pub fn coach_hide(&mut self) {
        self.coach_reset();
    }

    /// 把当前句子交给后台线程。只有完结的句子（打了句号或 `（）`）才发请求，
    /// 未完结的不发——省 token，等用户明确表示打完了再翻译。
    fn coach_submit(&mut self) {
        let Some(service) = &self.coach else {
            return;
        };
        if !self.coach_context.is_completed() {
            return;
        }
        let config = &self.applied_coach;
        let request = qingjian_coach::EnglishCoachRequest {
            text: self.coach_context.text().to_owned(),
            previous: self
                .coach_context
                .previous()
                .map(|previous| tail_chars(previous, config.previous_chars)),
            learner_level: config.learner_level().to_owned(),
            max_phrases: config.max_phrases,
            sentence_id: self.coach_context.sentence_id(),
            version: self.coach_context.version(),
            completed: self.coach_context.is_completed(),
        };
        tracing::debug!(
            sentence_id = request.sentence_id,
            version = request.version,
            completed = request.completed,
            chars = request.text.chars().count(),
            "English Coach 提交"
        );
        service.submit(request);
        self.coach_monitor.start();
    }

    /// 轮询定时器回调：结果到了先校验句子号，同一句的旧版本仍显示
    /// （差几个字翻译不会完全错，总比空白好）；不同句的旧结果才丢弃。
    pub fn poll_coach(&mut self) {
        let mut any = false;
        while let Some(outcome) = self.coach.as_mut().and_then(|s| s.poll()) {
            any = true;
            let current = (
                self.coach_context.sentence_id(),
                self.coach_context.version(),
            );
            if outcome.sentence_id != self.coach_context.sentence_id() {
                tracing::debug!(
                    sentence_id = outcome.sentence_id,
                    version = outcome.version,
                    ?current,
                    "English Coach 结果已过期（不同句），丢弃"
                );
                continue;
            }
            match outcome.result {
                Ok(response) => {
                    // 被动学习：面板上看到的每条短语记一次曝光（不推进复习间隔）
                    for phrase in &response.phrases {
                        self.phrase_book
                            .record_exposure(&phrase.phrase, &phrase.meaning);
                    }
                    let frame = CoachFrame {
                        english: response.english,
                        phrases: response.phrases,
                    };
                    tracing::debug!(
                        english = %frame.english,
                        phrases = frame.phrases.len(),
                        "English Coach 显示"
                    );
                    self.coach_frame = Some(frame);
                    self.coach_reposition();
                }
                Err(error) => tracing::debug!(%error, "English Coach 失败，面板保留上一次结果"),
            }
        }
        if !any {
            if self.coach_monitor.expired() {
                self.coach_monitor.stop();
            }
            return;
        }
        self.coach_monitor.stop();
    }

    /// 面板还显示着就把它摆到候选窗下方；候选窗收着就贴光标行。
    /// 每次候选窗重画（光标动了）都调一次，英文跟着走。
    pub fn coach_reposition(&mut self) {
        let Some(frame) = self.coach_frame.clone() else {
            return;
        };
        let below = if self.window.is_visible() {
            self.window.frame()
        } else {
            self.anchor
        };
        self.coach_panel.show(&frame, below);
    }

    /// 复习窗口里答了一题：对当前短语记一次复习，推进到下一条。
    pub fn review_answer(&mut self, quality: u8) {
        if let Some(phrase) = self.review.current_phrase() {
            let phrase = phrase.to_owned();
            self.phrase_book.record_review(&phrase, quality);
            tracing::debug!(%phrase, quality, "复习答题");
        }
        self.review.advance();
    }
}

#[cfg(test)]
mod tests {
    use super::tail_chars;

    #[test]
    fn tail_keeps_the_last_chars() {
        assert_eq!(tail_chars("我稍后回复你", 2), "复你");
        assert_eq!(tail_chars("短句", 10), "短句");
        assert_eq!(tail_chars("我稍后", 0), "");
    }
}
