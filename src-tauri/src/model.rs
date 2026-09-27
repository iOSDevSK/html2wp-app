use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub source_name: String,
    pub kind: String,
    pub created_at: String,
    pub updated_at: String,
    pub phase: String,
    #[serde(default)]
    pub last_step: Option<String>,
    pub revision: u32,
    pub thread_id: Option<String>,
    pub pages: Vec<Page>,
    pub gates: Vec<Gate>,
    pub artifacts: Vec<Artifact>,
    pub preview: Option<Preview>,
    pub runtime_image: String,
    pub plugin_commit: String,
    pub reporting: String,
    pub last_error: Option<String>,
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default)]
    pub conversion_approval_required: bool,
    #[serde(default)]
    pub archived: bool,
    /// Resulting theme type: "html" (classic HTML WordPress theme, the
    /// default and what every project saved before this field meant) or
    /// "gutenberg" (native block theme, manifest html2wp/2).
    #[serde(default = "default_target")]
    pub target: String,
    /// Codex model for this project's conversations. None follows the
    /// default chosen in Settings until the project's first turn pins it.
    #[serde(default)]
    pub model: Option<String>,
    /// Reasoning effort for `model`; None uses the model's saved default.
    #[serde(default)]
    pub effort: Option<String>,
    /// Flash conversion: the theme built in a minute or two from a drafted
    /// manifest, without visual checks. Chosen before the conversion starts.
    #[serde(default)]
    pub flash: bool,
}
pub const TARGETS: &[&str] = &["html", "gutenberg", "astro", "h2g"];
pub fn default_target() -> String {
    "html".into()
}
pub fn valid_target(target: &str) -> Result<String> {
    if TARGETS.contains(&target) { Ok(target.into()) } else { Err("Unknown output type. Choose an HTML theme, a Gutenberg theme or Gutenberg from an HTML theme.".into()) }
}
/// The types a project can be given now; "gutenberg" stays readable for
/// projects of earlier releases, but this release converts a site to an HTML
/// theme (or an Astro 5 project) and makes Gutenberg from that HTML theme.
pub fn offered_target(target: &str) -> Result<String> {
    if target == "gutenberg" { return Err("A Gutenberg block theme is made from the HTML theme: convert the site to an HTML WordPress theme, then import its ZIP as \"Gutenberg from an HTML theme\".".into()); }
    valid_target(target)
}
impl Project {
    pub fn gutenberg(&self) -> bool {
        self.target == "gutenberg"
    }
    /// Output is the Astro 5 project only: no WordPress theme, no service
    /// conversion, no WordPress preview.
    pub fn astro_only(&self) -> bool {
        self.target == "astro"
    }
    /// The pipeline the runner follows up to the Astro build is the HTML one;
    /// an Astro-only project simply stops there.
    pub fn runner_target(&self) -> &str {
        if self.astro_only() { "html" } else { &self.target }
    }
    /// The output type in the owner's words, for goals and messages.
    pub fn target_label(&self) -> &'static str {
        if self.gutenberg() { "native Gutenberg block theme" } else if self.astro_only() { "Astro 5 project" } else if self.from_theme() { "native Gutenberg block theme from an HTML theme" } else { "HTML WordPress theme" }
    }
    /// Gutenberg from an HTML theme: the input is a theme html2wp made, the
    /// html2wp-to-gutenberg skill makes the block theme (crate::h2g).
    pub fn from_theme(&self) -> bool {
        self.target == "h2g"
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub key: String,
    pub title: String,
    pub kind: String,
    pub reviewed_revision: Option<u32>,
    pub note: String,
    pub image: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gate {
    pub name: String,
    pub status: String,
    pub detail: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub revision: u32,
    pub filename: String,
    pub sha256: String,
    pub created_at: String,
    pub kind: String,
    pub reviewed: bool,
    /// Which checks this revision's files passed before packaging: "full"
    /// (every gate and the owner's review; empty on files saved before this
    /// field existed) or "quick" (an edit built after the quick checks only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checks: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub url: String,
    pub username: String,
    pub running: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub project_id: String,
    pub role: String,
    pub text: String,
    pub created_at: String,
    /// A button the chat shows with the message ("exports": open Exports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub project_id: String,
    pub label: String,
    pub status: String,
    pub created_at: String,
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub type Result<T> = std::result::Result<T, String>;
pub fn value<T: Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}
