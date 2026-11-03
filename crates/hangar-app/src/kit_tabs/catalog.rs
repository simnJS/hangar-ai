//! What the control bus reads of a kit's tabs for `kit_tab_list` and
//! `kit_tab_open` (P18's `KitTabCatalog`): `kitTabListing`, `kitTabToOpen`
//! and `readKitTabParams` of kitTabs.ts, with their words.

use hangar_core::kits::KitInfo;
use serde_json::Value;

use super::tabs::{kit_tab_listing, kit_tab_to_open, read_kit_tab_params};
use crate::control::port::{KitTabCatalog, KitTabToOpen};
use crate::state::types::{Settings, Workspace};

#[derive(Debug, Default)]
pub struct Catalog;

impl KitTabCatalog for Catalog {
    fn listing(&self, ws: &Workspace, settings: &Settings, installed: &[KitInfo]) -> Vec<Value> {
        kit_tab_listing(ws, settings, installed)
    }

    fn to_open(
        &self,
        ws: &Workspace,
        settings: &Settings,
        installed: &[KitInfo],
        kit: &str,
        tab: &str,
        raw: Option<&Value>,
    ) -> Result<KitTabToOpen, String> {
        let (_, tab) = kit_tab_to_open(ws, settings, installed, kit, tab)?;
        // hangar-core's MCP server held the values to their rules first,
        // where a rule cannot backtrack for ever (`Tab::param_refusal`).
        let params = read_kit_tab_params(tab, raw)?;
        Ok(KitTabToOpen {
            title: tab.title.clone(),
            singleton: tab.singleton,
            params,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::kit_tabs::testing::{entry, installed, kit, param, tab};

    fn ws() -> Workspace {
        Workspace {
            name: "Game".into(),
            kits: Some(vec![entry("ci-kit", &["notify"])]),
            ..Workspace::default()
        }
    }

    #[test]
    fn opens_a_tab_with_its_title_its_kind_and_the_params_held_to_its_rules() {
        let mut run = tab("run", "Run", "run.html", &["notify"]);
        run.singleton = true;
        run.params.insert("run".into(), param(Some("[0-9]+")));
        let kits = [kit("ci-kit", vec![run])];
        let open = Catalog
            .to_open(
                &ws(),
                &Settings::default(),
                &kits,
                "ci-kit",
                "run",
                Some(&json!({ "run": "4567" })),
            )
            .unwrap();
        assert_eq!(open.title, "Run");
        assert!(open.singleton);
        assert_eq!(open.params.get("run").map(String::as_str), Some("4567"));
        let refused = Catalog
            .to_open(
                &ws(),
                &Settings::default(),
                &kits,
                "ci-kit",
                "run",
                Some(&json!({ "run": "x" })),
            )
            .unwrap_err();
        assert!(refused.contains("must match its rule"), "{refused}");
    }

    #[test]
    fn lists_nothing_of_a_kit_the_workspace_does_not_have() {
        let rows = Catalog.listing(&ws(), &Settings::default(), &installed());
        assert!(rows.is_empty());
    }
}
