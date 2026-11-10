// Imports
use adw::prelude::*;
use gettextrs::gettext;
use gtk4::{Align, Button, Entry, Grid, Label, glib, glib::clone};

/// A template-function to create a simple dialog widget for an action:
///
/// ```text
/// -------------------------
/// |         <Label>       |
/// | <        Entry      > |
/// | <Cancel>      <Apply> |
/// -------------------------
/// ```
///
/// Just create the `apply` button and the label.
/// Everything else is done in this function.
///
/// Only `ApplyButton` and the dialog are returned because you likely want to
/// apply a connection to them.
///
/// This is a dialog inside the window and not a popover on purpose. A popover is a popup surface
/// that the compositor dismisses when something else gets touched, and that includes the on-screen
/// keyboard, so the keyboard would close as soon as it gets used.
pub(crate) fn create_entry_dialog(entry: &Entry, label: &Label) -> (Button, adw::Dialog) {
    let grid = Grid::builder()
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .column_spacing(18)
        .row_spacing(6)
        .build();
    let cancel_button = Button::builder()
        .halign(Align::Start)
        .label(gettext("Cancel"))
        .build();

    let apply_button = Button::builder()
        .halign(Align::End)
        .label(gettext("Apply"))
        .build();
    apply_button.add_css_class("suggested-action");

    grid.attach(label, 0, 0, 2, 1);
    grid.attach(entry, 0, 1, 2, 1);
    grid.attach(&cancel_button, 0, 2, 1, 1);
    grid.attach(&apply_button, 1, 2, 1, 1);

    let dialog = adw::Dialog::builder()
        .child(&grid)
        .follows_content_size(true)
        .presentation_mode(adw::DialogPresentationMode::Floating)
        .build();
    dialog.set_focus(Some(entry));

    cancel_button.connect_clicked(clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));

    // listen for enter key on entry
    entry.connect_activate(clone!(
        #[weak]
        apply_button,
        move |_| {
            apply_button.emit_clicked();
        }
    ));

    (apply_button, dialog)
}
