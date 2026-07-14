// Imports
use crate::appwindow::RnAppWindow;
use gettextrs::gettext;
use gtk4::{
    Button, CompositeTemplate, Label, ListBox, ListBoxRow, MenuButton, PopoverMenu, Widget, gio,
    glib, glib::clone, prelude::*, subclass::prelude::*,
};
use rnote_engine::Camera;

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate)]
    #[template(resource = "/com/github/flxzt/rnote/ui/canvasmenu.ui")]
    pub(crate) struct RnCanvasMenu {
        #[template_child]
        pub(crate) menubutton: TemplateChild<MenuButton>,
        #[template_child]
        pub(crate) popovermenu: TemplateChild<PopoverMenu>,
        #[template_child]
        pub(crate) menu_model: TemplateChild<gio::MenuModel>,
        #[template_child]
        pub(crate) zoom_in_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) zoom_out_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) zoom_reset_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) zoom_fit_width_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) zoom_real_width_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) rotation_reset_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) fixedsize_quickactions_box: TemplateChild<gtk4::Box>,
        #[template_child]
        pub(crate) bookmarks_listbox: TemplateChild<ListBox>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnCanvasMenu {
        const NAME: &'static str = "RnCanvasMenu";
        type Type = super::RnCanvasMenu;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for RnCanvasMenu {
        fn constructed(&self) {
            self.parent_constructed();

            self.menubutton
                .get()
                .set_popover(Some(&self.popovermenu.get()));
        }

        fn dispose(&self) {
            self.dispose_template();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RnCanvasMenu {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.popovermenu.get().present();
        }
    }
}

glib::wrapper! {
    pub(crate) struct RnCanvasMenu(ObjectSubclass<imp::RnCanvasMenu>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for RnCanvasMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl RnCanvasMenu {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn popovermenu(&self) -> PopoverMenu {
        self.imp().popovermenu.get()
    }

    pub(crate) fn fixedsize_quickactions_box(&self) -> gtk4::Box {
        self.imp().fixedsize_quickactions_box.get()
    }

    pub(crate) fn init(&self, appwindow: &RnAppWindow) {
        self.refresh_zoom_reset_label(Camera::ZOOM_DEFAULT);
        self.refresh_rotation_reset_label(0.0);

        let bookmarks_placeholder = Label::builder()
            .label(gettext("No bookmarks"))
            .margin_top(6)
            .margin_bottom(6)
            .css_classes(["dim-label"])
            .build();
        self.imp()
            .bookmarks_listbox
            .set_placeholder(Some(&bookmarks_placeholder));

        // The bookmarks of the active tab can change while the popover is closed,
        // so refresh the list every time it gets opened.
        self.imp().popovermenu.connect_map(clone!(
            #[weak(rename_to=canvasmenu)]
            self,
            #[weak]
            appwindow,
            move |_| {
                canvasmenu.refresh_bookmarks(&appwindow);
            }
        ));
    }

    pub(crate) fn refresh_zoom_reset_label(&self, zoom: f64) {
        self.imp()
            .zoom_reset_button
            .set_label(format!("{:.0}%", 100.0 * zoom).as_str());
    }

    pub(crate) fn refresh_rotation_reset_label(&self, rotation: f64) {
        self.imp()
            .rotation_reset_button
            .set_label(format!("{:.0}°", rotation.to_degrees()).as_str());
    }

    /// Rebuild the bookmarks list from the engine of the active tab.
    pub(crate) fn refresh_bookmarks(&self, appwindow: &RnAppWindow) {
        let listbox = self.imp().bookmarks_listbox.get();

        while let Some(row) = listbox.row_at_index(0) {
            listbox.remove(&row);
        }

        let Some(canvas) = appwindow.active_tab_canvas() else {
            return;
        };
        let bookmarks = canvas.engine_ref().bookmarks_in_document_order();
        let format_height = canvas.engine_ref().document.config.format.height();

        for (engine_index, bookmark) in bookmarks {
            let page_no = if format_height > 0.0 {
                ((bookmark.pos[1] / format_height).floor() as i64 + 1).max(1)
            } else {
                1
            };
            let label = Label::builder()
                .label(format!(
                    "{} {} · {:.0}%",
                    gettext("Page"),
                    page_no,
                    bookmark.zoom * 100.0
                ))
                .hexpand(true)
                .xalign(0.0)
                .build();

            let remove_button = Button::builder()
                .icon_name("trash-symbolic")
                .tooltip_text(gettext("Remove Bookmark"))
                .css_classes(["flat"])
                .build();
            remove_button.set_action_name(Some("win.remove-bookmark-at"));
            remove_button.set_action_target_value(Some(&(engine_index as u32).to_variant()));

            let row_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(12)
                .margin_start(6)
                .build();
            row_box.append(&label);
            row_box.append(&remove_button);

            let row = ListBoxRow::builder().child(&row_box).build();
            row.set_action_name(Some("win.jump-to-bookmark"));
            row.set_action_target_value(Some(&(engine_index as u32).to_variant()));
            listbox.append(&row);
        }
    }
}
