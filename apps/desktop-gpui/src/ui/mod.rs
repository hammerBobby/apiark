pub mod activity_bar;
pub mod auth_editor;
pub mod form_data_editor;
pub mod json_view;
pub mod key_value_editor;
pub mod prelude;
pub mod request_panel;
pub mod response_panel;
pub mod side_panel;
pub mod status_bar;
pub mod tab_bar;
pub mod url_bar;

pub use activity_bar::ActivityBar;
pub use auth_editor::AuthEditor;
pub use form_data_editor::FormDataEditor;
pub use key_value_editor::{KeyValueEditor, KvTarget};
pub use request_panel::RequestPanel;
pub use response_panel::ResponsePanel;
pub use side_panel::SidePanel;
pub use status_bar::StatusBar;
pub use tab_bar::TabBar;
pub use url_bar::UrlBar;

use gpui_kit::{App, Entity};

use crate::services::{collection as collection_service, WorkspaceService};
use crate::state::{AppState, Overlay, TabsState, ToastKind};

/// Save the active tab to its collection YAML file. Shared by the toolbar
/// button and the global Ctrl/Cmd+S action.
pub fn save_active_tab(tabs: &Entity<TabsState>, app_state: &Entity<AppState>, cx: &mut App) {
    let Some(tab) = tabs.read(cx).active_tab().cloned() else {
        return;
    };
    let Some(path) = tab.file_path.clone() else {
        let name = if tab.name.trim().is_empty() {
            "Untitled Request".to_string()
        } else {
            tab.name.clone()
        };
        let location = tab
            .collection_path
            .clone()
            .or_else(|| {
                let state = app_state.read(cx);
                state
                    .collection_path
                    .clone()
                    .or_else(|| state.collections.first().map(|node| node.path().to_string()))
            })
            .unwrap_or_else(WorkspaceService::collections_root);
        app_state.update(cx, |state, cx| {
            state.set_overlay(Overlay::SaveAs { name, location }, cx);
        });
        return;
    };
    let path_ref = std::path::Path::new(&path);
    let result = match tab.case_name.as_deref() {
        Some(case_name) => {
            collection_service::save_request_case(path_ref, case_name, &tab.to_request_file())
                .map(|parent| {
                    tabs.update(cx, |state, cx| {
                        if let Some(active) = state.active_tab_mut() {
                            active.cases = parent.cases;
                        }
                        state.mark_clean(cx);
                    });
                })
        }
        None => collection_service::write_request(path_ref, &tab.to_request_file()),
    };
    match result {
        Ok(()) => {
            tabs.update(cx, |state, cx| state.mark_clean(cx));
            app_state.update(cx, |state, cx| {
                state.show_toast(
                    format!("Saved {}", path.rsplit(['/', '\\']).next().unwrap_or(&path)),
                    ToastKind::Success,
                    cx,
                );
            });
        }
        Err(e) => {
            app_state.update(cx, |state, cx| {
                state.show_toast(format!("Save failed: {e}"), ToastKind::Error, cx);
            });
        }
    }
}
