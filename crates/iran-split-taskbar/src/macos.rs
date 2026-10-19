//! Two-line menu bar title. A plain status-item title stays one proportional
//! line, so the item changes width every time the rate changes.
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, Message};
use objc2_app_kit::{
    NSApplication, NSButton, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSImage, NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSView, NSWindow,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSRect, NSString};

/// True when `AppKit` calls are legal on this thread.
#[must_use]
pub fn menu_bar_is_main_thread() -> bool {
    MainThreadMarker::new().is_some()
}

/// Draws `label` on `BiFlow`'s status item. `None` clears the title.
///
/// Returns whether that status item was found.
#[must_use]
pub fn apply_menu_bar_label(label: Option<&str>) -> bool {
    let Some(marker) = MainThreadMarker::new() else {
        return false;
    };
    let Some(button) = status_button(marker) else {
        return false;
    };
    let tip = NSString::from_str("BiFlow");
    button.setToolTip(Some(&tip));
    if let Some(label) = label {
        let font = NSFont::monospacedSystemFontOfSize_weight(9.0, 0.0);
        let style = NSMutableParagraphStyle::new();
        style.setMaximumLineHeight(9.0);
        style.setLineSpacing(-2.0);
        let text = NSString::from_str(label);
        let attrs = attributes(&font, &style, &NSColor::labelColor());
        // SAFETY: the dictionary uses the font, paragraph, and foreground
        // attribute names, and each value is the matching AppKit object.
        let title = unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &text,
                Some(&attrs),
            )
        };
        button.setFont(Some(&font));
        button.setAttributedTitle(&title);
        show_logo(&button, false);
    } else {
        button.setTitle(&NSString::from_str(""));
        show_logo(&button, true);
    }
    fit_click_target(&button);
    true
}

fn attributes(
    font: &NSFont,
    style: &NSMutableParagraphStyle,
    color: &NSColor,
) -> objc2::rc::Retained<NSDictionary<NSString, AnyObject>> {
    // SAFETY: Apple's attribute-name constants are non-null for the process lifetime.
    let keys: [&NSString; 3] = unsafe {
        [
            NSFontAttributeName,
            NSParagraphStyleAttributeName,
            NSForegroundColorAttributeName,
        ]
    };
    let objects = [object_ref(font), object_ref(style), object_ref(color)];
    NSDictionary::from_slices(&keys, &objects)
}

fn object_ref(object: &impl Message) -> &AnyObject {
    // SAFETY: every `Message` is an Objective-C object, which is the layout
    // `AnyObject` describes. The reference stays borrowed from `object`.
    unsafe { &*std::ptr::from_ref(object).cast::<AnyObject>() }
}

fn status_button(marker: MainThreadMarker) -> Option<objc2::rc::Retained<NSButton>> {
    let mut buttons = Vec::new();
    for window in windows(marker) {
        if let Some(root) = window.contentView() {
            collect_buttons(&root, &mut buttons);
        }
    }
    buttons
        .iter()
        .find(|button| tooltip(button).as_deref() == Some("BiFlow"))
        .cloned()
        .or_else(|| (buttons.len() == 1).then(|| buttons.remove(0)))
}

fn windows(marker: MainThreadMarker) -> Vec<objc2::rc::Retained<NSWindow>> {
    let list = NSApplication::sharedApplication(marker).windows();
    (0..list.count())
        .map(|index| list.objectAtIndex(index))
        .collect()
}

fn collect_buttons(view: &NSView, buttons: &mut Vec<objc2::rc::Retained<NSButton>>) {
    let children = view.subviews();
    for index in 0..children.count() {
        let child = children.objectAtIndex(index);
        if child.class().name() == c"TaoTrayTarget" {
            // SAFETY: the overlay is retained for this call and is only read
            // on the main thread. Its superview is the status button.
            let Some(parent) = (unsafe { child.superview() }) else {
                continue;
            };
            if let Ok(button) = parent.downcast::<NSButton>() {
                buttons.push(button);
            }
        } else {
            collect_buttons(&child, buttons);
        }
    }
}

fn tooltip(button: &NSButton) -> Option<String> {
    button.toolTip().map(|tip| tip.to_string())
}

fn show_logo(button: &NSButton, visible: bool) {
    std::thread_local! {
        static MENU_BAR_LOGO: std::cell::RefCell<Option<objc2::rc::Retained<NSImage>>> =
            const { std::cell::RefCell::new(None) };
    }
    if visible {
        let restored = MENU_BAR_LOGO.with(|logo| logo.borrow().clone());
        if let Some(image) = restored {
            button.setImage(Some(&image));
        }
        return;
    }
    if let Some(image) = button.image() {
        MENU_BAR_LOGO.with(|logo| *logo.borrow_mut() = Some(image));
    }
    button.setImage(None);
}

fn fit_click_target(button: &NSButton) {
    // A leftover overlay frame can keep `sizeToFit` from shrinking the item.
    place_overlay(button, NSRect::ZERO);
    button.sizeToFit();
    place_overlay(button, button.bounds());
}

fn place_overlay(button: &NSButton, frame: NSRect) {
    let children = button.subviews();
    for index in 0..children.count() {
        let child = children.objectAtIndex(index);
        if child.class().name() == c"TaoTrayTarget" {
            child.setFrame(frame);
        }
    }
}
