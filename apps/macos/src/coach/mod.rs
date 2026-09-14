//! English Coach 面板：候选窗下方常驻的一句英文（第一层）与值得学的短语（第二层），
//! 自绘 NSPanel，不吃鼠标、不抢焦点。
//!
//! AI 永远不阻塞输入：这里只有「有内容就显示、没有就收起」。

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSAttributedStringNSStringDrawing, NSBackingStoreType, NSBezierPath, NSColor, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSPanel, NSScreen, NSView,
    NSWindowCollectionBehavior, NSWindowLevel, NSWindowStyleMask,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use qingjian_coach::LearningPhrase;
use qingjian_platform::ThemeMode;

use crate::candidates::theme::Theme;

/// `kCGPopUpMenuWindowLevel`，与候选窗同级。
const POPUP_MENU_LEVEL: NSWindowLevel = 101;

/// 面板与候选窗（或光标行）之间的间隙。
const GAP: f64 = 4.0;

/// 面板里一行文字与面板边的内边距。
const PADDING: f64 = 8.0;

/// 英文行与短语行之间的间距。
const PHRASE_GAP: f64 = 2.0;

/// 面板的一帧：一句英文 + 值得学的短语。
#[derive(Debug, Clone, Default)]
pub struct CoachFrame {
    /// 地道英文表达。
    pub english: String,

    /// 值得学的短语（第二层）。
    pub phrases: Vec<LearningPhrase>,
}

impl CoachFrame {
    pub fn is_empty(&self) -> bool {
        self.english.is_empty()
    }
}

/// 面板的内容视图：圆角背景 + 一行英文，一帧一画。
struct ViewIvars {
    /// 当前显示的一帧。
    frame: std::cell::RefCell<CoachFrame>,

    /// 主题（字体与颜色沿用候选窗的）。
    theme: Theme,
}

define_class!(
    // SAFETY: NSView 允许子类化；没有实现 Drop。
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ViewIvars]
    struct CoachView;

    impl CoachView {
        /// 用左上角为原点的坐标系。
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

impl CoachView {
    fn new(mtm: MainThreadMarker, theme: Theme) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ViewIvars {
            frame: std::cell::RefCell::new(CoachFrame::default()),
            theme,
        });
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    /// 存好这一帧并返回需要的视图尺寸：英文一行画不下就换行，最多两行；每个短语一行。
    fn set_frame(&self, frame: &CoachFrame, max_width: f64) -> NSSize {
        *self.ivars().frame.borrow_mut() = frame.clone();
        let theme = &self.ivars().theme;
        let english_text = format!("🇬🇧 {}", frame.english);
        let english_width = self.attributed(&english_text, theme).size().width;
        let english_lines = ((english_width / max_width).ceil() as usize).clamp(1, 2);
        let text_height = self.attributed("x", theme).size().height;
        let annotation_height = self.annotation_attributed("x", theme).size().height;
        let phrase_width = frame
            .phrases
            .iter()
            .map(|phrase| {
                self.annotation_attributed(&phrase_line(phrase), theme)
                    .size()
                    .width
            })
            .fold(0.0_f64, f64::max);
        let content_width = english_width.max(phrase_width).min(max_width);
        let mut height = text_height * english_lines as f64;
        if !frame.phrases.is_empty() {
            height += PHRASE_GAP + annotation_height * frame.phrases.len() as f64;
        }
        NSSize::new(content_width + PADDING * 2.0, height + PADDING * 2.0)
    }

    fn draw(&self) {
        let theme = &self.ivars().theme;
        let bounds = self.bounds();
        theme.background.set();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            bounds,
            theme.corner_radius,
            theme.corner_radius,
        )
        .fill();
        let frame = self.ivars().frame.borrow();
        let english_text = format!("🇬🇧 {}", frame.english);
        let english_attr = self.attributed(&english_text, theme);
        let english_size = english_attr.size();
        let max_width = bounds.size.width - PADDING * 2.0;
        let english_lines = ((english_size.width / max_width).ceil() as usize).clamp(1, 2);
        let text_height = english_size.height;
        let annotation_height = self.annotation_attributed("x", theme).size().height;
        // 英文行：垂直居中在上半区
        let english_y = PADDING;
        english_attr.drawAtPoint(NSPoint::new(PADDING, english_y));
        // 短语行：英文下方，每行一条
        if !frame.phrases.is_empty() {
            let mut y = english_y + text_height * english_lines as f64 + PHRASE_GAP;
            for phrase in &frame.phrases {
                let line = self.annotation_attributed(&phrase_line(phrase), theme);
                line.drawAtPoint(NSPoint::new(PADDING, y));
                y += annotation_height;
            }
        }
    }

    fn attributed(&self, text: &str, theme: &Theme) -> Retained<NSAttributedString> {
        attributed_string(text, &theme.text_font, &theme.text_color)
    }

    /// 短语行用小一号的释义字体与译文颜色，比英文行弱化。
    fn annotation_attributed(&self, text: &str, theme: &Theme) -> Retained<NSAttributedString> {
        attributed_string(text, &theme.annotation_font, &theme.gloss_color)
    }
}

/// 一条短语画成一行：`phrase — meaning  [CEFR]`；CEFR 留空就不带尾。
fn phrase_line(phrase: &LearningPhrase) -> String {
    let mut line = format!("{} — {}", phrase.phrase, phrase.meaning);
    if !phrase.cefr.is_empty() {
        line.push_str(&format!("  [{}]", phrase.cefr));
    }
    line
}

fn attributed_string(text: &str, font: &NSFont, color: &NSColor) -> Retained<NSAttributedString> {
    // SAFETY: 只读 AppKit 导出的属性名常量
    let (keys, objects): (Vec<&NSString>, Vec<&objc2::runtime::AnyObject>) = unsafe {
        (
            vec![NSFontAttributeName, NSForegroundColorAttributeName],
            vec![font, color],
        )
    };
    let attributes = NSDictionary::from_slices(&keys, &objects);
    // SAFETY: 属性名常量合法
    unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(text), &attributes) }
}

pub struct CoachPanel {
    /// 面板本体。不在当前 Space 时会整个换新（与候选窗同样的坑）。
    panel: Retained<NSPanel>,

    /// 内容视图；换面板时要搬到新面板上。
    view: Retained<CoachView>,

    /// 当前外观（跟随系统时为 `None`）；换面板时要重设。
    appearance: Option<Retained<NSAppearance>>,

    mtm: MainThreadMarker,
}

impl CoachPanel {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let view = CoachView::new(mtm, Theme::system_default());
        let panel = build_panel(mtm, &view);
        Self {
            panel,
            view,
            appearance: None,
            mtm,
        }
    }

    /// 显示一帧。`below` 是要贴在它下方的矩形（候选窗或光标行）。
    pub fn show(&mut self, frame: &CoachFrame, below: NSRect) {
        if frame.is_empty() {
            self.hide();
            return;
        }
        let size = self.view.set_frame(frame, self.max_width());
        let origin = self.place(size, below);
        self.panel.setFrame_display(NSRect::new(origin, size), true);
        self.order_front_on_active_space();
    }

    /// 最宽取主屏可见宽度的一半；取不到就给个宽裕的默认值。
    fn max_width(&self) -> f64 {
        NSScreen::mainScreen(self.mtm)
            .map(|screen| screen.visibleFrame().size.width * 0.5)
            .unwrap_or(400.0)
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    /// 外观：跟随系统时不指定，否则强制浅色 / 深色。
    pub fn set_theme(&mut self, mode: ThemeMode) {
        // SAFETY: 只读 AppKit 导出的常量名
        let name = unsafe {
            match mode {
                ThemeMode::System => None,
                ThemeMode::Light => Some(NSAppearanceNameAqua),
                ThemeMode::Dark => Some(NSAppearanceNameDarkAqua),
            }
        };
        let appearance = name.and_then(NSAppearance::appearanceNamed);
        self.panel.setAppearance(appearance.as_deref());
        self.appearance = appearance;
    }

    /// 排到最前，并确认真在当前 Space 上；不在就换一块新面板（见候选窗同名方法的注释）。
    fn order_front_on_active_space(&mut self) {
        self.panel.orderFrontRegardless();
        if self.panel.isOnActiveSpace() {
            return;
        }
        let frame_rect = self.panel.frame();
        self.panel.orderOut(None);
        let panel = build_panel(self.mtm, &self.view);
        panel.setAppearance(self.appearance.as_deref());
        panel.setFrame_display(frame_rect, true);
        panel.orderFrontRegardless();
        self.panel = panel;
        tracing::warn!("English Coach 面板不在当前 Space，已换新面板");
    }

    /// 面板左下角坐标：贴在 `below` 下方；下方放不下放上方；不出 `below` 所在的那块屏幕。
    fn place(&self, size: NSSize, below: NSRect) -> NSPoint {
        let Some(screen) = screen_containing(self.mtm, below.origin) else {
            return NSPoint::new(below.origin.x, below.origin.y - GAP - size.height);
        };
        let min_x = screen.origin.x;
        let max_x = (screen.origin.x + screen.size.width - size.width).max(min_x);
        let x = below.origin.x.clamp(min_x, max_x);
        let below_y = below.origin.y - GAP - size.height;
        let above_y = below.origin.y + below.size.height + GAP;
        let top = screen.origin.y + screen.size.height;
        let y = if below_y >= screen.origin.y {
            below_y
        } else if above_y + size.height <= top {
            above_y
        } else {
            screen.origin.y
        };
        let max_y = (top - size.height).max(screen.origin.y);
        NSPoint::new(x, y.clamp(screen.origin.y, max_y))
    }
}

/// 建一块面板并把内容视图装进去：无边框、不抢焦点、透明背景带阴影、不吃鼠标，与候选窗同层级。
fn build_panel(mtm: MainThreadMarker, view: &CoachView) -> Retained<NSPanel> {
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        mtm.alloc::<NSPanel>(),
        NSRect::new(NSPoint::ZERO, NSSize::new(200.0, 30.0)),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setIgnoresMouseEvents(true);
    // NSPanel 缺省在应用失活时自动隐藏；输入法进程从来不是前台应用，不能靠这个
    panel.setHidesOnDeactivate(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );
    // 层级最后设，别被前面任何一项覆盖
    panel.setLevel(POPUP_MENU_LEVEL);
    panel.setContentView(Some(view));
    panel
}

/// 包含 `point` 的那块屏幕的可见区域；哪块都不包含返回 `None`。
fn screen_containing(mtm: MainThreadMarker, point: NSPoint) -> Option<NSRect> {
    NSScreen::screens(mtm)
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            point.x >= frame.origin.x
                && point.x < frame.origin.x + frame.size.width
                && point.y >= frame.origin.y
                && point.y < frame.origin.y + frame.size.height
        })
        .map(|screen| screen.visibleFrame())
}
