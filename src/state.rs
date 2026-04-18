use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl std::str::FromStr for ProjectSize {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "small" => Ok(ProjectSize::Small),
            "medium" => Ok(ProjectSize::Medium),
            "large" => Ok(ProjectSize::Large),
            _ => Err(()),
        }
    }
}

impl ProjectSize {
    pub fn max_files(&self) -> usize {
        match self {
            ProjectSize::Small => 50,
            ProjectSize::Medium => 200,
            ProjectSize::Large => 1000,
        }
    }

    /// Java gets deeper package hierarchy; other languages use shallower defaults.
    pub fn max_depth(&self, language: &str) -> usize {
        let is_java = language.to_lowercase() == "java";
        match self {
            ProjectSize::Small => if is_java { 4 } else { 2 },
            ProjectSize::Medium => if is_java { 4 } else { 3 },
            ProjectSize::Large => if is_java { 7 } else { 5 },
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectSize::Small => "small",
            ProjectSize::Medium => "medium",
            ProjectSize::Large => "large",
        }
    }
}

/// Result of a background build job.
#[derive(Debug)]
pub enum BuildJob {
    Running { pid: u32 },
    Done { exit_code: i32, stdout: String, stderr: String },
}

#[derive(Debug)]
pub struct AppState {
    /// Active project directory (None until create_project or use_project is called).
    pub project_dir: Option<PathBuf>,
    pub language: Option<String>,
    pub max_files: usize,
    pub max_depth: usize,
    pub size: ProjectSize,
    pub project_name: Option<String>,
    /// Base directory for all projects: $PRJ_DIR if set, otherwise CWD at startup.
    pub base_dir: PathBuf,
    /// Absolute path to git binary (from $GIT_CMD or default /usr/bin/git).
    pub git_cmd: PathBuf,
    /// Background build jobs keyed by job_id.
    pub jobs: HashMap<String, BuildJob>,
}

impl AppState {
    pub fn new() -> Self {
        let base_dir = std::env::var("PRJ_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().expect("Cannot determine CWD"));

        let git_cmd = std::env::var("GIT_CMD")
            .ok()
            .and_then(|s| {
                let p = PathBuf::from(&s);
                if p.is_absolute() {
                    Some(p)
                } else {
                    eprintln!("Warning: GIT_CMD '{}' is not an absolute path — using default", s);
                    None
                }
            })
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));

        AppState {
            project_dir: None,
            language: None,
            max_files: 200,
            max_depth: 3,
            size: ProjectSize::Medium,
            project_name: None,
            base_dir,
            git_cmd,
            jobs: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    pub fn has_project(&self) -> bool {
        self.project_dir.is_some()
    }
}
