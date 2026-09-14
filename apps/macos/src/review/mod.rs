//! English Coach 复习窗口：菜单栏「复习…」打开，列出到期短语，用户点「认识 / 不认识」走 SRS。
//!
//! 复用偏好设置窗口的建窗模式（NSWindow 子类 + ActivationPolicy 切换），不抢输入焦点、
//! 关窗切回纯后台。被动学习（面板曝光）和主动学习（这里答题）严格分离（design §17）。

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBezelStyle, NSButton,
    NSFont, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSInteger, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::host;

/// 按钮 tag：1 = 认识，0 = 不认识。
const TAG_KNOW: NSInteger = 1;
const TAG_DONT: NSInteger = 0;

/// 认识 → SM-2 的 5 分，不认识 → 2 分（< 3 算答错，间隔回 1）。
const QUALITY_KNOW: u8 = 5;
const QUALITY_DONT: u8 = 2;

/// 到期的一批短语：`(phrase, meaning, mastery)`。
pub type DueList = Vec<(String, String, f32)>;

/// 复习窗口。
pub struct ReviewWindow {
    panel: Retained<ReviewPanel>,
    phrase_field: Retained<NSTextField>,
    meaning_field: Retained<NSTextField>,
    _target: Retained<ReviewTarget>,
    due: DueList,
    index: usize,
}

impl ReviewWindow {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let target = ReviewTarget::new(mtm);
        let content = NSRect::new(NSPoint::ZERO, NSSize::new(360.0, 160.0));

        let phrase_field = label(mtm, 20.0);
        phrase_field.setFrame(NSRect::new(
            NSPoint::new(20.0, 100.0),
            NSSize::new(320.0, 28.0),
        ));
        let meaning_field = label(mtm, 14.0);
        meaning_field.setFrame(NSRect::new(
            NSPoint::new(20.0, 70.0),
            NSSize::new(320.0, 22.0),
        ));

        let dont = button(mtm, "不认识", TAG_DONT, &target);
        dont.setFrame(NSRect::new(
            NSPoint::new(20.0, 20.0),
            NSSize::new(130.0, 32.0),
        ));
        let know = button(mtm, "认识", TAG_KNOW, &target);
        know.setFrame(NSRect::new(
            NSPoint::new(210.0, 20.0),
            NSSize::new(130.0, 32.0),
        ));

        let view = NSView::initWithFrame(mtm.alloc(), content);
        view.addSubview(&phrase_field);
        view.addSubview(&meaning_field);
        view.addSubview(&dont);
        view.addSubview(&know);

        let panel = ReviewPanel::new(mtm, content);
        panel.setTitle(&NSString::from_str("English Coach 复习"));
        panel.setContentView(Some(&view));

        Self {
            panel,
            phrase_field,
            meaning_field,
            _target: target,
            due: Vec::new(),
            index: 0,
        }
    }

    /// 装载到期短语并打开窗口；没有就显示「暂无需要复习的内容」。
    pub fn show(&mut self, due: DueList) {
        self.due = due;
        self.index = 0;
        self.render();
        self.panel.present();
    }

    /// 当前短语的英文，答题时回传给 `PhraseBook::record_review`。
    pub fn current_phrase(&self) -> Option<&str> {
        self.due
            .get(self.index)
            .map(|(phrase, _, _)| phrase.as_str())
    }

    /// 下一条；答完最后一条显示「复习完成」。
    pub fn advance(&mut self) {
        self.index += 1;
        self.render();
    }

    fn render(&self) {
        if let Some((phrase, meaning, _)) = self.due.get(self.index) {
            self.set_field(&self.phrase_field, phrase);
            self.set_field(&self.meaning_field, meaning);
        } else if self.due.is_empty() {
            self.set_field(&self.phrase_field, "暂无需要复习的内容");
            self.set_field(&self.meaning_field, "");
        } else {
            self.set_field(&self.phrase_field, "复习完成！");
            self.set_field(&self.meaning_field, "");
        }
    }

    fn set_field(&self, field: &NSTextField, text: &str) {
        field.setStringValue(&NSString::from_str(text));
    }
}

fn label(mtm: MainThreadMarker, size: f64) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(mtm.alloc(), NSRect::ZERO);
    let font = NSFont::systemFontOfSize(size);
    field.setEditable(false);
    field.setSelectable(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setFont(Some(&font));
    field
}

fn button(
    mtm: MainThreadMarker,
    title: &str,
    tag: NSInteger,
    target: &ReviewTarget,
) -> Retained<NSButton> {
    let btn = NSButton::initWithFrame(mtm.alloc(), NSRect::ZERO);
    btn.setTitle(&NSString::from_str(title));
    btn.setBezelStyle(NSBezelStyle::Push);
    unsafe {
        btn.setTarget(Some(target));
        btn.setAction(Some(sel!(answer:)));
    }
    btn.setTag(tag);
    btn
}

define_class!(
    // SAFETY: NSWindow 允许子类化；没有实现 Drop。
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    /// 复习窗口的 NSWindow：关窗时切回 Prohibited，和偏好设置窗口一样。
    struct ReviewPanel;

    impl ReviewPanel {
        #[unsafe(method(close))]
        fn close(&self) {
            let mtm = MainThreadMarker::from(self);
            NSApplication::sharedApplication(mtm)
                .setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
            let _: () = unsafe { msg_send![super(self), close] };
        }
    }

    unsafe impl NSObjectProtocol for ReviewPanel {}
);

impl ReviewPanel {
    pub fn new(mtm: MainThreadMarker, content: NSRect) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        let this: Retained<Self> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: content,
                styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        unsafe { this.setReleasedWhenClosed(false) };
        this
    }

    pub fn present(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        self.makeKeyAndOrderFront(None);
    }
}

define_class!(
    // SAFETY: NSObject 没有子类化要求；没有实现 Drop。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    /// 复习窗口按钮的 target：「认识 / 不认识」汇到 host。
    struct ReviewTarget;

    impl ReviewTarget {
        #[unsafe(method(answer:))]
        fn answer(&self, sender: Option<&AnyObject>) {
            let tag: NSInteger = sender
                .map(|s| unsafe { msg_send![s, tag] })
                .unwrap_or(0);
            let quality = if tag == TAG_KNOW { QUALITY_KNOW } else { QUALITY_DONT };
            host::with(|h| h.review_answer(quality));
        }
    }

    unsafe impl NSObjectProtocol for ReviewTarget {}
);

impl ReviewTarget {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}
