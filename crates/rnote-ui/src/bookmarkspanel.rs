// Imports
use crate::strokecontentpaintable::StrokeContentPaintable;
use crate::workspacebrowser::widgethelper;
use crate::{RnAppWindow, RnCanvas};
use gettextrs::gettext;
use gtk4::{
    Align, Button, CompositeTemplate, ContentFit, Entry, Label, ListBox, ListBoxRow, Picture,
    Widget, glib, glib::clone, pango, prelude::*, subclass::prelude::*,
};
use rnote_engine::document::Bookmark;
use std::cell::{Cell, RefCell};
use std::time::Duration;

/// The width the bookmark previews are painted with (in pixels, before the hi-dpi scale factor).
const PREVIEW_PAINT_WIDTH: f64 = 450.0;
/// The time to wait after the document was modified before the previews get repainted.
const PREVIEW_REFRESH_TIMEOUT: Duration = Duration::from_millis(800);

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate)]
    #[template(resource = "/com/github/flxzt/rnote/ui/bookmarkspanel.ui")]
    pub(crate) struct RnBookmarksPanel {
        pub(crate) appwindow: glib::WeakRef<RnAppWindow>,
        /// The canvas and its bookmarks the list was built for, to skip rebuilding it when nothing changed.
        pub(crate) built_for: RefCell<Option<(glib::WeakRef<RnCanvas>, Vec<Bookmark>)>>,
        /// The preview paintables of the rows, with their bookmark index.
        pub(crate) previews: RefCell<Vec<(usize, StrokeContentPaintable)>>,
        pub(crate) previews_stale: Cell<bool>,
        pub(crate) preview_refresh_source: RefCell<Option<glib::SourceId>>,

        #[template_child]
        pub(crate) listbox: TemplateChild<ListBox>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnBookmarksPanel {
        const NAME: &'static str = "RnBookmarksPanel";
        type Type = super::RnBookmarksPanel;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for RnBookmarksPanel {
        fn constructed(&self) {
            self.parent_constructed();
        }

        fn dispose(&self) {
            if let Some(source) = self.preview_refresh_source.take() {
                source.remove();
            }
            self.dispose_template();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RnBookmarksPanel {
        fn map(&self) {
            self.parent_map();
            // The list is not kept up to date while the panel is hidden.
            self.obj().refresh();
        }

        fn unmap(&self) {
            self.parent_unmap();
            if let Some(appwindow) = self.appwindow.upgrade() {
                set_highlighted_bookmark(&appwindow, None);
            }
        }
    }
}

glib::wrapper! {
    pub(crate) struct RnBookmarksPanel(ObjectSubclass<imp::RnBookmarksPanel>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for RnBookmarksPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl RnBookmarksPanel {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn init(&self, appwindow: &RnAppWindow) {
        self.imp().appwindow.set(Some(appwindow));

        let placeholder = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(6)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(12)
            .margin_end(12)
            .build();
        placeholder.append(
            &Label::builder()
                .label(gettext("No Bookmarks"))
                .css_classes(["heading"])
                .build(),
        );
        placeholder.append(
            &Label::builder()
                .label(gettext("Add one for the current view with Ctrl+Shift+B"))
                .wrap(true)
                .justify(gtk4::Justification::Center)
                .css_classes(["dim-label"])
                .build(),
        );
        self.imp().listbox.set_placeholder(Some(&placeholder));
    }

    /// Update the list from the bookmarks of the active tab.
    ///
    /// Does nothing while the panel is hidden, it gets refreshed when it is shown again.
    pub(crate) fn refresh(&self) {
        let imp = self.imp();
        if !self.is_mapped() {
            return;
        }
        let Some(appwindow) = imp.appwindow.upgrade() else {
            return;
        };
        let canvas = appwindow.active_tab_canvas();
        let bookmarks = canvas
            .as_ref()
            .map(|c| c.engine_ref().document.bookmarks.clone())
            .unwrap_or_default();

        let unchanged =
            imp.built_for
                .borrow()
                .as_ref()
                .is_some_and(|(built_canvas, built_bookmarks)| {
                    built_canvas.upgrade() == canvas && *built_bookmarks == bookmarks
                });
        if unchanged {
            if imp.previews_stale.get() {
                self.refresh_previews();
            }
            return;
        }

        self.rebuild(&appwindow, canvas.as_ref());
        imp.built_for.replace(canvas.map(|c| {
            let weak = glib::WeakRef::new();
            weak.set(Some(&c));
            (weak, bookmarks)
        }));
    }

    /// Mark the previews as outdated, e.g. after the document was modified.
    ///
    /// They get repainted after a timeout when the panel is shown, or else when it is shown the next time.
    pub(crate) fn previews_outdated(&self) {
        let imp = self.imp();
        imp.previews_stale.set(true);
        if !self.is_mapped() {
            return;
        }
        if let Some(source) = imp.preview_refresh_source.take() {
            source.remove();
        }
        imp.preview_refresh_source
            .replace(Some(glib::timeout_add_local_once(
                PREVIEW_REFRESH_TIMEOUT,
                clone!(
                    #[weak(rename_to=panel)]
                    self,
                    move || {
                        panel.imp().preview_refresh_source.take();
                        panel.refresh();
                    }
                ),
            )));
    }

    fn refresh_previews(&self) {
        let imp = self.imp();
        imp.previews_stale.set(false);
        let Some(canvas) = imp.appwindow.upgrade().and_then(|a| a.active_tab_canvas()) else {
            return;
        };
        let scale_factor = self.scale_factor().max(1) as f64;
        for (index, paintable) in imp.previews.borrow().iter() {
            if let Some(content) = canvas.engine_ref().extract_bookmark_preview_content(*index) {
                set_preview_content(paintable, content, scale_factor);
            }
        }
    }

    fn rebuild(&self, appwindow: &RnAppWindow, canvas: Option<&RnCanvas>) {
        let imp = self.imp();
        let listbox = imp.listbox.get();

        // The bookmark indices might change, so any current highlight becomes stale.
        set_highlighted_bookmark(appwindow, None);
        imp.previews.borrow_mut().clear();
        imp.previews_stale.set(false);
        while let Some(row) = listbox.row_at_index(0) {
            listbox.remove(&row);
        }

        let Some(canvas) = canvas else {
            return;
        };
        let bookmarks = canvas.engine_ref().bookmarks_in_document_order();
        let format_height = canvas.engine_ref().document.config.format.height();
        let scale_factor = self.scale_factor().max(1) as f64;

        for (engine_index, bookmark) in bookmarks {
            let page_no = if format_height > 0.0 {
                ((bookmark.pos[1] / format_height).floor() as i64 + 1).max(1)
            } else {
                1
            };
            let location_text = format!(
                "{} {} · {:.0}%",
                gettext("Page"),
                page_no,
                bookmark.zoom * 100.0
            );

            let paintable = StrokeContentPaintable::new();
            paintable.set_draw_background(true);
            paintable.set_draw_pattern(true);
            if let Some(content) = canvas
                .engine_ref()
                .extract_bookmark_preview_content(engine_index)
            {
                set_preview_content(&paintable, content, scale_factor);
            }
            let picture = Picture::builder()
                .paintable(&paintable)
                .content_fit(ContentFit::Contain)
                .can_shrink(true)
                .build();
            let preview = BookmarkPreview::new(&picture);
            imp.previews
                .borrow_mut()
                .push((engine_index, paintable.clone()));

            let labels_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Vertical)
                .hexpand(true)
                .valign(Align::Center)
                .build();
            let title_label = Label::builder()
                .label(if bookmark.name.is_empty() {
                    location_text.clone()
                } else {
                    bookmark.name.clone()
                })
                .xalign(0.0)
                .ellipsize(pango::EllipsizeMode::End)
                .build();
            labels_box.append(&title_label);
            if !bookmark.name.is_empty() {
                labels_box.append(
                    &Label::builder()
                        .label(location_text)
                        .xalign(0.0)
                        .ellipsize(pango::EllipsizeMode::End)
                        .css_classes(["caption", "dim-label"])
                        .build(),
                );
            }

            let rename_button = Button::builder()
                .icon_name("edit-symbolic")
                .tooltip_text(gettext("Rename Bookmark"))
                .valign(Align::Center)
                .css_classes(["flat"])
                .build();
            rename_button.connect_clicked(clone!(
                #[weak(rename_to=panel)]
                self,
                #[strong(rename_to=name)]
                bookmark.name,
                move |button| {
                    panel.rename_bookmark_dialog(button, engine_index, &name);
                }
            ));

            let remove_button = Button::builder()
                .icon_name("trash-symbolic")
                .tooltip_text(gettext("Remove Bookmark"))
                .valign(Align::Center)
                .css_classes(["flat"])
                .build();
            remove_button.set_action_name(Some("win.remove-bookmark-at"));
            remove_button.set_action_target_value(Some(&(engine_index as u32).to_variant()));

            let footer_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(6)
                .build();
            footer_box.append(&labels_box);
            footer_box.append(&rename_button);
            footer_box.append(&remove_button);

            let row_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Vertical)
                .spacing(6)
                .margin_top(9)
                .margin_bottom(6)
                .margin_start(9)
                .margin_end(6)
                .build();
            row_box.append(&preview);
            row_box.append(&footer_box);

            let row = ListBoxRow::builder().child(&row_box).build();
            row.set_action_name(Some("win.jump-to-bookmark"));
            row.set_action_target_value(Some(&(engine_index as u32).to_variant()));

            // Indicate the bookmark location on the canvas while its row is hovered.
            let motion_controller = gtk4::EventControllerMotion::new();
            motion_controller.connect_enter(clone!(
                #[weak]
                appwindow,
                move |_, _, _| {
                    set_highlighted_bookmark(&appwindow, Some(engine_index));
                }
            ));
            motion_controller.connect_leave(clone!(
                #[weak]
                appwindow,
                move |_| {
                    set_highlighted_bookmark(&appwindow, None);
                }
            ));
            row.add_controller(motion_controller);

            listbox.append(&row);
        }
    }

    /// Shows a small dialog popover on `parent` for renaming the bookmark at `engine_index`.
    fn rename_bookmark_dialog(
        &self,
        parent: &impl IsA<Widget>,
        engine_index: usize,
        current_name: &str,
    ) {
        let entry = Entry::builder()
            .text(current_name)
            .placeholder_text(gettext("Bookmark name"))
            .build();
        let label = Label::builder()
            .margin_bottom(12)
            .halign(Align::Center)
            .label(gettext("Rename Bookmark"))
            .width_chars(24)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        label.add_css_class("title-4");

        let (apply_button, popover) = widgethelper::create_entry_dialog(&entry, &label);
        popover.set_parent(parent);
        popover.connect_closed(|popover| {
            glib::idle_add_local_once(clone!(
                #[weak]
                popover,
                move || {
                    popover.unparent();
                }
            ));
        });

        apply_button.connect_clicked(clone!(
            #[weak(rename_to=panel)]
            self,
            #[weak]
            popover,
            #[weak]
            entry,
            move |_| {
                let Some(appwindow) = panel.imp().appwindow.upgrade() else {
                    return;
                };
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    let widget_flags = canvas
                        .engine_mut()
                        .rename_bookmark_at(engine_index, entry.text().trim().to_owned());
                    if let Some(widget_flags) = widget_flags {
                        appwindow.handle_widget_flags(widget_flags, &canvas);
                    }
                }
                popover.popdown();
                panel.refresh();
            }
        ));

        popover.popup();
        entry.grab_focus();
    }
}

fn set_preview_content(
    paintable: &StrokeContentPaintable,
    content: rnote_engine::engine::StrokeContent,
    scale_factor: f64,
) {
    let Some(size) = content.size() else {
        return;
    };
    // Keep the aspect ratio of the content, the paintable scales the axes separately.
    let paint_width = PREVIEW_PAINT_WIDTH * scale_factor;
    paintable.set_paint_max_width(paint_width);
    paintable.set_paint_max_height(paint_width * size[1] / size[0].max(1.0));
    paintable.set_stroke_content(content);
}

mod preview_imp {
    use super::*;

    #[derive(Default, Debug)]
    pub(crate) struct BookmarkPreview {}

    #[glib::object_subclass]
    impl ObjectSubclass for BookmarkPreview {
        const NAME: &'static str = "RnBookmarkPreview";
        type Type = super::BookmarkPreview;
        type ParentType = Widget;
    }

    impl ObjectImpl for BookmarkPreview {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for BookmarkPreview {
        fn request_mode(&self) -> gtk4::SizeRequestMode {
            gtk4::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk4::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let (min, nat, _, _) = child.measure(orientation, for_size);
            if orientation == gtk4::Orientation::Vertical && for_size >= 0 {
                // Scrolled windows only hand out the minimum height,
                // so require the full height the picture needs at this width.
                (nat, nat, -1, -1)
            } else {
                (min, nat, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    /// Shows a bookmark preview picture at the full available width, keeping its aspect ratio.
    pub(crate) struct BookmarkPreview(ObjectSubclass<preview_imp::BookmarkPreview>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl BookmarkPreview {
    fn new(picture: &Picture) -> Self {
        let preview: Self = glib::Object::new();
        picture.set_parent(&preview);
        preview
    }
}

/// Set the bookmark whose indicator gets drawn on the canvas of the active tab,
/// or None to hide all bookmark indicators.
pub(crate) fn set_highlighted_bookmark(appwindow: &RnAppWindow, index: Option<usize>) {
    let Some(canvas) = appwindow.active_tab_canvas() else {
        return;
    };
    let widget_flags = canvas.engine_mut().set_highlighted_bookmark(index);
    appwindow.handle_widget_flags(widget_flags, &canvas);
}
