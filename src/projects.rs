use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The project a working directory belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectRef {
    /// Normalized main-repository root; the grouping key.
    pub key: String,
    /// Last path component of the root.
    pub name: String,
    /// Root as reported by the filesystem, for display.
    pub root: String,
    /// Worktree root when the cwd is inside a linked git worktree.
    pub worktree: Option<String>,
}

/// Grouping key for a path: `/` separators, no trailing slash, lowercase on Windows.
pub fn normalize(path: &str) -> String {
    let mut s = path.replace('\\', "/");
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if cfg!(windows) {
        s = s.to_lowercase();
    }
    s
}

/// Walks up from `cwd` to the nearest `.git`; linked worktrees resolve to their main repository.
pub fn resolve(cwd: &Path) -> ProjectRef {
    if cwd.as_os_str().is_empty() {
        return ProjectRef {
            key: String::new(),
            name: "(unknown)".into(),
            root: String::new(),
            worktree: None,
        };
    }
    for dir in cwd.ancestors() {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            return project(dir, None);
        }
        if dot_git.is_file() {
            return match main_repo_of_worktree(&dot_git) {
                Some(main) => project(&main, Some(dir)),
                None => project(dir, None),
            };
        }
    }
    project(cwd, None)
}

/// A linked worktree's `.git` file reads `gitdir: <main>/.git/worktrees/<name>`.
/// Submodules point at `.git/modules/<name>` instead and stay their own project.
fn main_repo_of_worktree(dot_git: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dot_git).ok()?;
    let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    let gitdir = if gitdir.is_relative() {
        dot_git.parent()?.join(gitdir)
    } else {
        gitdir
    };
    let worktrees = gitdir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    Some(worktrees.parent()?.parent()?.to_path_buf())
}

fn project(root: &Path, worktree: Option<&Path>) -> ProjectRef {
    let display = root.to_string_lossy().to_string();
    ProjectRef {
        key: normalize(&display),
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| display.clone()),
        root: display,
        worktree: worktree.map(|w| w.to_string_lossy().to_string()),
    }
}

/// Memoizes `resolve` per cwd string for the life of the process.
#[derive(Default)]
pub struct Resolver {
    cache: HashMap<String, ProjectRef>,
}

impl Resolver {
    pub fn resolve(&mut self, cwd: &str) -> ProjectRef {
        self.cache
            .entry(cwd.to_string())
            .or_insert_with(|| resolve(Path::new(cwd)))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize("/a/b/"), "/a/b");
        if cfg!(windows) {
            assert_eq!(
                normalize(r"C:\Projects\Home\Wygląda\"),
                "c:/projects/home/wygląda"
            );
        }
    }

    #[test]
    fn subdirectory_resolves_to_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("alpha");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("src/deep")).unwrap();
        let p = resolve(&repo.join("src/deep"));
        assert_eq!(p.name, "alpha");
        assert_eq!(p.key, normalize(&repo.to_string_lossy()));
        assert_eq!(p.worktree, None);
    }

    #[test]
    fn worktree_nests_under_main_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        fs::create_dir_all(main.join(".git/worktrees/feat")).unwrap();
        let wt = tmp.path().join("main-feat");
        fs::create_dir_all(&wt).unwrap();
        let gitdir = main.join(".git/worktrees/feat");
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", gitdir.to_string_lossy().replace('\\', "/")),
        )
        .unwrap();
        let p = resolve(&wt);
        assert_eq!(p.name, "main");
        assert_eq!(p.key, normalize(&main.to_string_lossy()));
        assert_eq!(
            p.worktree.as_deref().map(normalize),
            Some(normalize(&wt.to_string_lossy()))
        );
    }

    #[test]
    fn submodule_is_its_own_project() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("parent/libs/sub");
        fs::create_dir_all(tmp.path().join("parent/.git/modules/sub")).unwrap();
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".git"), "gitdir: ../../.git/modules/sub\n").unwrap();
        assert_eq!(resolve(&sub).name, "sub");
    }

    #[test]
    fn plain_directory_is_its_own_project() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("notes");
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(resolve(&dir).name, "notes");
    }

    #[test]
    fn empty_cwd_is_unknown_project() {
        let p = Resolver::default().resolve("");
        assert_eq!(p.name, "(unknown)");
        assert_eq!(p.key, "");
    }
}
