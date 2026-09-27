//! User-requested local downloads; never reads shared auth or other projects.
use crate::{model::*, store::Store};
use serde_json::{json, Value};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}};

fn destination(store: &Store, dest: &Path) -> Result<PathBuf> {
    let parent = dest.parent().ok_or("Choose a destination folder")?.canonicalize().map_err(err)?;
    let target = parent.join(dest.file_name().ok_or("Choose a filename")?);
    let root = store.root.canonicalize().map_err(err)?;
    if target.starts_with(root) || target.is_symlink() || target.is_dir() {
        return Err("Choose a regular file outside the application's private data".into());
    }
    Ok(target)
}
fn atomic_archive(dest: &Path, write: impl FnOnce(&mut fs::File) -> Result<()>) -> Result<()> {
    let temp = dest.with_file_name(format!(".html2wp-{}.tmp", id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new().write(true).read(true).create_new(true).open(&temp).map_err(err)?;
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; file.set_permissions(fs::Permissions::from_mode(0o600)).map_err(err)?; }
        write(&mut file)?;
        file.sync_all().map_err(err)?;
        fs::rename(&temp, dest).map_err(err)
    })();
    if result.is_err() { let _ = fs::remove_file(temp); }
    result
}
pub fn original_path(store: &Store, p: &Project) -> PathBuf {
    store.root.join("private/original-imports").join(format!("{}.zip", p.id))
}
/// Keep new ZIP uploads byte-for-byte, outside every agent mount.
pub fn retain_original(store: &Store, p: &Project, source: &Path) -> Result<()> {
    if source.is_file() {
        let path = original_path(store, p);
        fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
        atomic_archive(&path, |file| { let mut input=fs::File::open(source).map_err(err)?; std::io::copy(&mut input,file).map_err(err)?; Ok(()) })?;
    }
    Ok(())
}
pub fn original(store: &Store, p: &Project, dest: &Path) -> Result<Value> {
    let dest=destination(store,dest)?;
    let saved=original_path(store,p);
    if saved.is_file() && !saved.is_symlink() {
        atomic_archive(&dest,|file| {std::io::copy(&mut fs::File::open(saved).map_err(err)?,file).map_err(err)?;Ok(())})?;
        return Ok(json!({"exactArchive":true}));
    }
    let input=store.path(&p.id)?.join("input");
    if !input.is_dir() || input.is_symlink() {return Err("The original imported files are unavailable".into());}
    atomic_archive(&dest, |file| {
        let mut zip=zip::ZipWriter::new(file);
        let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for entry in walkdir::WalkDir::new(&input).follow_links(false).sort_by_file_name() {
            let entry=entry.map_err(err)?;
            if entry.depth()==0 {continue;}
            if entry.file_type().is_symlink() {return Err("The original input contains a symbolic link".into());}
            let name=entry.path().strip_prefix(&input).map_err(err)?.to_str().ok_or("Unsupported filename")?.replace('\\',"/");
            if entry.file_type().is_dir() {zip.add_directory(format!("{name}/"),options).map_err(err)?;}
            else if entry.file_type().is_file() {
                zip.start_file(name,options).map_err(err)?;
                std::io::copy(&mut fs::File::open(entry.path()).map_err(err)?,&mut zip).map_err(err)?;
            }
        }
        zip.finish().map_err(err)?;Ok(())
    })?;
    Ok(json!({"exactArchive":false}))
}
/// Report text may contain credentials copied into a failed command. Redact lines, not substrings.
pub fn redact(text: &str) -> String {
    let mut private_key=false;
    text.lines().map(|line| {
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {private_key=true;}
        if private_key {if line.contains("-----END"){private_key=false;}return "[redacted private key]".to_string();}
        let lower=line.to_lowercase();
        let sensitive=["password", "passwd", "authorization", "bearer ", "api_key", "api-key", "apikey", "access_token", "refresh_token", "secret", "cookie", "licence", "license_key", "token", "key=", "sk-", "h2wp-lic"].iter().any(|key|lower.contains(key));
        let credential_url=line.split_whitespace().any(|word|word.contains("://")&&word.contains('@'));
        if sensitive || credential_url {"[redacted credential-bearing line]".to_string()} else {line.to_string()}
    }).collect::<Vec<_>>().join("\n")
}
fn scrub(value: &mut Value) {
    match value {
        Value::Object(map)=>for (key,v) in map {let k=key.to_lowercase();if ["password","token","secret","cookie","authorization","licence","licensekey","apikey","api_key","credential"].iter().any(|x|k.contains(x)){*v=json!("[redacted]")}else{scrub(v)}},
        Value::Array(items)=>for item in items{scrub(item)},
        Value::String(s)=>*s=redact(s), _=>{}
    }
}
pub fn diagnostics(store: &Store,p: &Project,dest: &Path)->Result<Value>{
    let dest=destination(store,dest)?;
    let root=store.path(&p.id)?;
    let mut summary=json!({"schema":"h2wp-diagnostics/1","capturedAt":now(),"appVersion":env!("CARGO_PKG_VERSION"),"project":p,"activity":store.activities(&p.id)?,"messages":store.diagnostic_messages(&p.id)?});
    scrub(&mut summary);
    let summary_bytes=serde_json::to_vec_pretty(&summary).map_err(err)?;
    if summary_bytes.len()>8*1024*1024{return Err("Diagnostic summary is too large".into());}
    let mut skipped=Vec::new();let mut count=0;
    atomic_archive(&dest,|file|{
        let mut zip=zip::ZipWriter::new(file);
        let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("diagnostics.json",options).map_err(err)?;
        zip.write_all(&summary_bytes).map_err(err)?;
        let mut total=summary_bytes.len() as u64;
        for area in ["workspace","out",".app"]{
            let base=root.join(area);
            if base.is_symlink(){continue;}
            for entry in walkdir::WalkDir::new(&base).follow_links(false).max_depth(4).into_iter().filter_entry(|e|e.depth()==0||(!e.file_name().to_string_lossy().starts_with('.') && !matches!(e.file_name().to_str(),Some("source"|"input"|"static-src"|"input-untouched"|"astro-project"|"theme"|"node_modules"|".git")))){
                let Ok(entry)=entry else{continue};
                if !entry.file_type().is_file(){continue;}
                let name=entry.file_name().to_string_lossy();
                let rel=entry.path().strip_prefix(&root).map_err(err)?.to_string_lossy().replace('\\',"/");
                let allowed=matches!(name.as_ref(),"progress.json"|"result.json"|"report.json"|"status.json"|"quick-check.json"|"CONVERSION-REPORT.md"|"VERIFICATION.md"|"verification-summary.json"|"source-assets-report.json"|"prerender-report.json"|"route-inventory.json"|"astro-coverage.json"|"theme-report.json"|"model-history.json"|"tool-events.jsonl"|".h2wp-run-context.json") || name.ends_with(".log");
                let relative=entry.path().strip_prefix(&base).map_err(err)?;
                let top=relative.components().next().and_then(|c|c.as_os_str().to_str()).unwrap_or("");
                let diagnostic_dir=matches!(top,"install-theme"|"verify-wp"|"smoke-editor"|"prerender"|"visual-review"|"theme-patches"|"repairs");
                let permitted=(area=="workspace" && matches!(relative.to_str(),Some("h2g/VERIFICATION.md"|"h2g/verification-summary.json"|"h2g/compare/status.json")))
                    || (area==".app" && matches!(name.as_ref(),"model-history.json"|"tool-events.jsonl"|"tool-events.previous.log"))
                    || (area!=".app" && (relative.components().count()==1 || diagnostic_dir));
                if !allowed || !permitted {continue;}
                let len=entry.metadata().map_err(err)?.len();
                if len>5*1024*1024 || total+len>30*1024*1024 {skipped.push(rel);continue;}
                let mut bytes=Vec::new();fs::File::open(entry.path()).map_err(err)?.take(5*1024*1024+1).read_to_end(&mut bytes).map_err(err)?;
                if bytes.len()>5*1024*1024{skipped.push(rel);continue;}
                let text=String::from_utf8_lossy(&bytes);
                let clean=if let Ok(mut v)=serde_json::from_str::<Value>(&text){scrub(&mut v);serde_json::to_string_pretty(&v).map_err(err)?}else if name.ends_with(".jsonl"){text.lines().map(|line|if let Ok(mut v)=serde_json::from_str::<Value>(line){scrub(&mut v);v.to_string()}else{redact(line)}).collect::<Vec<_>>().join("\n")}else{redact(&text)};
                if total+clean.len() as u64>30*1024*1024{skipped.push(rel);continue;}
                total+=clean.len() as u64;
                zip.start_file(rel,options).map_err(err)?;zip.write_all(clean.as_bytes()).map_err(err)?;count+=1;
            }
        }
        zip.start_file("README.txt",options).map_err(err)?;
        write!(zip,"Diagnostic snapshot captured at {}. May include site URLs and conversation content. Known credential fields/lines are redacted; review before sharing. Chat includes at most 100 recent messages of up to 64 KB each. Original source, auth, licence, browser profiles and database dumps are excluded. A running conversion may change while this snapshot is taken.\nSkipped oversized files: {:?}\n",now(),skipped).map_err(err)?;
        zip.finish().map_err(err)?;Ok(())
    })?;
    Ok(json!({"files":count,"skipped":skipped}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_uses_input_and_preserves_uploaded_zip_bytes() {
        let dir=tempfile::tempdir().unwrap();let store=Store::open(dir.path().join("data")).unwrap();
        let p=crate::skill::tests::project();let root=store.path(&p.id).unwrap();
        fs::create_dir_all(root.join("input")).unwrap();fs::create_dir_all(root.join("source")).unwrap();
        fs::write(root.join("input/index.html"),"original").unwrap();fs::write(root.join("source/index.html"),"changed").unwrap();
        let dest=dir.path().join("original.zip");
        assert_eq!(original(&store,&p,&dest).unwrap()["exactArchive"],false);
        let mut zip=zip::ZipArchive::new(fs::File::open(&dest).unwrap()).unwrap();let mut body=String::new();zip.by_name("index.html").unwrap().read_to_string(&mut body).unwrap();assert_eq!(body,"original");
        let upload=dir.path().join("upload.zip");fs::copy(&dest,&upload).unwrap();retain_original(&store,&p,&upload).unwrap();
        fs::write(root.join("input/index.html"),"changed later").unwrap();
        assert_eq!(original(&store,&p,&dest).unwrap()["exactArchive"],true);
        assert_eq!(fs::read(dest).unwrap(),fs::read(upload).unwrap());
        assert!(original(&store,&p,&root.join("oops.zip")).is_err());
    }
    #[test]
    fn diagnostics_keep_failures_but_exclude_auth_and_source() {
        let dir=tempfile::tempdir().unwrap();let store=Store::open(dir.path().join("data")).unwrap();let p=crate::skill::tests::project();
        let ws=store.path(&p.id).unwrap().join("workspace");fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("report.json"),r#"{"error":"anchor failed","password":"never-export","token":"secret-value"}"#).unwrap();
        fs::create_dir_all(ws.join("static-src")).unwrap();fs::write(ws.join("static-src/report.json"),"source-secret").unwrap();
        fs::create_dir_all(ws.join("astro-project/src")).unwrap();fs::write(ws.join("astro-project/src/x.log"),"source-secret").unwrap();
        fs::write(ws.join(".h2wp-job.json"),"job-secret").unwrap();fs::write(ws.join("source.html"),"source-private").unwrap();
        let dest=dir.path().join("logs.zip");diagnostics(&store,&p,&dest).unwrap();
        let mut zip=zip::ZipArchive::new(fs::File::open(dest).unwrap()).unwrap();assert!(zip.by_name("workspace/.h2wp-job.json").is_err());
        assert!(zip.by_name("workspace/static-src/report.json").is_err());assert!(zip.by_name("workspace/astro-project/src/x.log").is_err());
        let mut text=String::new();zip.by_name("workspace/report.json").unwrap().read_to_string(&mut text).unwrap();
        assert!(text.contains("anchor failed"));assert!(!text.contains("never-export")&&!text.contains("secret-value"));
        for secret in ["curl --token opaque-secret","token: opaque-secret","GH_TOKEN=opaque-secret","http://user:opaque-secret@localhost/"] {assert!(!redact(secret).contains("opaque-secret"));}
        for _ in 0..105 {store.message(&p.id,"assistant","small").unwrap();}
        store.message(&p.id,"assistant",&"x".repeat(70000)).unwrap();
        store.message(&p.id,"assistant",&"ž".repeat(40000)).unwrap();
        assert_eq!(store.diagnostic_messages(&p.id).unwrap().len(),100);
        assert!(!redact("ok\nAuthorization: Bearer abc\npassword=xyz\nlast failure").contains("abc"));
    }
}
