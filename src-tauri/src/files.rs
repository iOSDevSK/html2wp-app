use crate::model::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
pub fn excluded(name: &str) -> bool {
    let n = name.to_lowercase();
    matches!(
        n.as_str(),
        "node_modules"
            | ".git"
            | ".svn"
            | ".hg"
            | ".codex"
            | ".agents"
            | ".claude"
            | ".ssh"
            | ".aws"
            | ".azure"
            | ".docker"
            | ".kube"
            | ".gnupg"
            | ".cache"
            | ".next"
            | "__macosx"
            | ".ds_store"
            | "agents.md"
            | "claude.md"
            | ".npmrc"
            | ".yarnrc"
            | ".yarnrc.yml"
            | ".netrc"
            | "credentials.json"
            | "credentials.yaml"
    ) || n.starts_with(".env")
        || n.starts_with("id_rsa")
        || n.starts_with("id_ed25519")
        || n.ends_with(".pem")
        || n.ends_with(".key")
        || n == ".h2wp-job.json"
}
pub fn relative_safe(path: &str) -> Result<PathBuf> {
    if path.is_empty() || path.contains('\\') || path.contains(':') || path.contains('\0') {
        return Err("Invalid relative path".into());
    }
    let p = Path::new(path);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("Path must stay inside the project".into());
    }
    Ok(p.to_path_buf())
}
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf> {
    let rel = relative_safe(relative)?;
    if rel
        .components()
        .any(|c| excluded(&c.as_os_str().to_string_lossy()))
    {
        return Err("This file is excluded from AI access".into());
    }
    let root = root.canonicalize().map_err(err)?;
    let mut ancestor = root.clone();
    for part in rel.components() {
        ancestor.push(part);
        if ancestor.is_symlink() {
            return Err("Symbolic links are not accessible to the assistant".into());
        }
    }
    let dest = root.join(rel);
    let mut p = dest.as_path();
    while !p.exists() {
        p = p.parent().ok_or("Invalid path")?
    }
    let canonical = p.canonicalize().map_err(err)?;
    if !canonical.starts_with(&root) {
        return Err("Path escapes the project".into());
    }
    Ok(dest)
}
pub fn copy_input(source: &Path, dest: &Path) -> Result<()> {
    if source.is_symlink() {
        return Err("Choose a real folder or ZIP, not a symbolic link".into());
    }
    fs::create_dir_all(dest).map_err(err)?;
    let mut count = 0;
    let mut total = 0u64;
    if source.is_dir() {
        for entry in walkdir::WalkDir::new(source)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !excluded(&e.file_name().to_string_lossy()))
        {
            let e = entry.map_err(err)?;
            if e.depth() == 0 {
                continue;
            }
            if e.file_type().is_symlink() {
                return Err(format!(
                    "Symbolic links are not imported: {}",
                    e.path().display()
                ));
            }
            let rel = e.path().strip_prefix(source).map_err(err)?;
            let target = dest.join(rel);
            if e.file_type().is_dir() {
                fs::create_dir_all(target).map_err(err)?;
            } else if e.file_type().is_file() {
                count += 1;
                total += e.metadata().map_err(err)?.len();
                limit(count, total)?;
                fs::copy(e.path(), target).map_err(err)?;
            } else {
                return Err("Special files cannot be imported".into());
            }
        }
    } else {
        let mut zip = zip::ZipArchive::new(fs::File::open(source).map_err(err)?)
            .map_err(|_| "Choose a project folder or a valid ZIP archive".to_string())?;
        if zip.len() > MAX_FILES {
            return Err("Archive has too many files".into());
        }
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(err)?;
            let raw = entry.name().trim_end_matches('/');
            if raw.is_empty() {
                continue;
            }
            let rel = relative_safe(raw)?;
            if rel
                .components()
                .any(|c| excluded(&c.as_os_str().to_string_lossy()))
            {
                continue;
            }
            if let Some(mode) = entry.unix_mode() {
                let kind = mode & 0o170000;
                if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                    return Err("Archive contains links or special files".into());
                }
            }
            count += 1;
            total = total.checked_add(entry.size()).ok_or("Archive too large")?;
            limit(count, total)?;
            let target = dest.join(rel);
            if entry.is_dir() {
                fs::create_dir_all(target).map_err(err)?;
                continue;
            }
            fs::create_dir_all(target.parent().ok_or("Invalid archive path")?).map_err(err)?;
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)
                .map_err(err)?;
            let actual =
                std::io::copy(&mut entry.by_ref().take(MAX_BYTES + 1), &mut out).map_err(err)?;
            if actual > MAX_BYTES {
                return Err("Archive exceeds the extraction limit".into());
            }
        }
    }
    if count == 0 {
        return Err("The project contains no importable files".into());
    }
    Ok(())
}
fn limit(count: usize, total: u64) -> Result<()> {
    if count > MAX_FILES || total > MAX_BYTES {
        Err("Project exceeds 100,000 files or 2 GB".into())
    } else {
        Ok(())
    }
}
pub fn input_root(root: &Path) -> PathBuf {
    if root.join("package.json").exists() || root.join("index.html").exists() {
        return root.into();
    }
    let dirs: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .collect();
    if dirs.len() == 1 && dirs[0].path().is_dir() {
        return dirs[0].path();
    }
    root.into()
}
/// The kind given to an Astro 5 project exported by html2wp (Exports → Astro
/// project): its built pages are the prerendered input and its sources are
/// the Astro project the converter would otherwise generate.
pub const ASTRO_KIND: &str = "Astro 5 project";
/// Where an html2wp Astro export carries the converter's astro-report.json.
pub const ASTRO_REPORT: &str = ".html2wp/astro-report.json";
/// An html2wp Astro 5 export: an Astro project with html2wp's page fragments
/// and its built site. A different Astro project is built and prerendered
/// like any other web app.
pub fn html2wp_astro(root: &Path) -> bool {
    let astro_dependency = fs::read(root.join("package.json")).ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .is_some_and(|v| v.pointer("/dependencies/astro").is_some() || v.pointer("/devDependencies/astro").is_some());
    astro_dependency
        && ["astro.config.mjs", "astro.config.ts", "astro.config.js"].iter().any(|c| root.join(c).is_file())
        && root.join("src/fragments/bodies").is_dir()
        && root.join("dist/index.html").is_file()
        && root.join(ASTRO_REPORT).is_file()
}
/// A static-site-generator project: its build writes every page as complete
/// HTML, so the pages are taken as a static site (runner step `ssg`) rather
/// than captured in a browser. Astro is the one generator known to pass.
pub const STATIC_SITE_ASTRO: &str = "Static site (Astro)";
fn static_site_generator(root: &Path) -> Option<&'static str> {
    let package: serde_json::Value = serde_json::from_slice(&fs::read(root.join("package.json")).ok()?).ok()?;
    package.pointer("/scripts/build")?.as_str()?;
    let astro = package.pointer("/dependencies/astro").is_some() || package.pointer("/devDependencies/astro").is_some();
    if !astro { return None; }
    // Server output renders on request: there are no pages to take.
    let ssr = ["astro.config.mjs", "astro.config.ts", "astro.config.js", "astro.config.mts", "astro.config.cjs"].iter()
        .filter_map(|c| fs::read_to_string(root.join(c)).ok())
        .map(|config| config.chars().filter(|c| !c.is_whitespace()).collect::<String>())
        .any(|config| config.contains("output:'server'") || config.contains("output:\"server\""));
    (!ssr).then_some(STATIC_SITE_ASTRO)
}
fn html_pages(root: &Path) -> Result<Vec<Page>> {
    let mut pages = vec![];
    for e in walkdir::WalkDir::new(root)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.path().extension().is_some_and(|x| x == "html") {
            let rel = e
                .path()
                .strip_prefix(root)
                .map_err(err)?
                .to_string_lossy()
                .to_string();
            pages.push(Page {
                key: rel.clone(),
                title: rel,
                kind: "page".into(),
                reviewed_revision: None,
                note: String::new(),
                image: None,
            })
        }
    }
    Ok(pages)
}
pub fn detect(root: &Path) -> Result<(String, Vec<Page>)> {
    let root = input_root(root);
    if html2wp_astro(&root) {
        let pages = html_pages(&root.join("dist"))?;
        if pages.is_empty() { return Err("The Astro project's dist folder has no built pages. Build it, then import it again.".into()); }
        return Ok((ASTRO_KIND.into(), pages));
    }
    if let Some(kind) = static_site_generator(&root) {
        return Ok((kind.into(), vec![]));
    }
    if root.join("package.json").exists() {
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("package.json")).map_err(err)?)
                .map_err(err)?;
        if value
            .pointer("/scripts/build")
            .and_then(|v| v.as_str())
            .is_none()
        {
            return Err("This project has no build script. Import a static export or add a build script first.".into());
        }
        return Ok(("Web app".into(), vec![]));
    }
    let pages = html_pages(&root)?;
    if pages.is_empty() {
        return Err("No HTML pages or supported buildable project found".into());
    }
    Ok(("Static HTML".into(), pages))
}
pub fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(err)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn traversal() {
        for p in ["../a", "/a", "a/../../b", "C:/x", "a\\b", ""] {
            assert!(relative_safe(p).is_err(), "{p}")
        }
    }
    #[test]
    fn secret_filter() {
        assert!(excluded(".env.production"));
        assert!(excluded("AGENTS.md"));
        assert!(!excluded("index.html"));
    }
    #[test]
    fn symlink_escape() {
        #[cfg(unix)]
        {
            let root = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink("/tmp", root.path().join("link")).unwrap();
            assert!(resolve(root.path(), "link/escape").is_err());
        }
    }
    #[test]
    fn rejects_zip_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("bad.zip");
        let mut z = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        z.start_file("../outside.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"bad").unwrap();
        z.finish().unwrap();
        assert!(copy_input(&archive, &temp.path().join("out")).is_err());
        assert!(!temp.path().join("outside.txt").exists());
    }
}
