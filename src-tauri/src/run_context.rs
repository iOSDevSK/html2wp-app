//! Measured execution metadata shared with the plugin's reports.
use crate::{model::*, store::Store};
use serde_json::{json, Value};
use std::{fs, io::Write, sync::Mutex};
static LOCK: Mutex<()> = Mutex::new(());
fn update(store:&Store,p:&Project,change:impl FnOnce(&mut Value))->Result<()> {
    let _guard=LOCK.lock().map_err(err)?;
    let dir=store.path(&p.id)?.join(".app");
    if dir.is_symlink(){return Err("Invalid metadata directory".into());}
    fs::create_dir_all(&dir).map_err(err)?;
    let path=dir.join("model-history.json");
    let began=store.setting(&format!("run-started:{}",p.id));
    let start=began.as_ref().filter(|s|chrono::DateTime::parse_from_rfc3339(s).is_ok()).cloned().unwrap_or_else(now);
    let run=began.unwrap_or_else(||p.created_at.clone());
    if path.is_symlink(){return Err("Invalid metadata file".into());}
    let mut doc:Value=fs::read(&path).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    if doc["schema"]!="h2wp-run-context/1" || !doc["models"].is_array() || !doc["turns"].is_array() || doc["run"]!=run || doc["revision"]!=p.revision {doc=json!({"schema":"h2wp-run-context/1","run":run,"revision":p.revision,"startedAt":start,"models":[],"turns":[]});}
    change(&mut doc);
    let temp=dir.join(format!("metadata-{}.tmp",id()));
    fs::write(&temp,serde_json::to_vec_pretty(&doc).map_err(err)?).map_err(err)?;
    fs::rename(temp,path).map_err(err)
}
pub fn selection(store:&Store,p:&Project,model:&str,effort:&str)->Result<()> {
    // Post-delivery chat does not rewrite the conversion's model/timing history.
    if p.phase=="deliverable_ready" {return Ok(());}
    update(store,p,|doc|{
        let models=doc["models"].as_array_mut().unwrap();
        if models.last().is_none_or(|m|m["model"]!=model||m["effort"]!=effort){models.push(json!({"model":model,"effort":effort,"at":now()}));}
    })
}
pub fn turn(store:&Store,p:&Project,id:&str,finished:bool)->Result<()> {
    if p.phase=="deliverable_ready" {return Ok(());}
    update(store,p,|doc|{
        let turns=doc["turns"].as_array_mut().unwrap();
        if let Some(row)=turns.iter_mut().find(|t|t["id"]==id){if finished {row["endedAt"]=json!(now());}}
        else if !finished {turns.push(json!({"id":id,"startedAt":now()}));}
    })
}
pub fn tool_log(store:&Store,p:&Project,cmd:&str,result:&Value)->Result<()> {
    let dir=store.path(&p.id)?.join(".app");
    if dir.is_symlink(){return Err("Invalid log directory".into());}
    fs::create_dir_all(&dir).map_err(err)?;
    let path=dir.join("tool-events.jsonl");
    if path.is_symlink(){return Err("Invalid log path".into());}
    // Keep a bounded recent log; the previous part remains downloadable.
    if fs::metadata(&path).is_ok_and(|m|m.len()>4*1024*1024){fs::rename(&path,dir.join("tool-events.previous.log")).map_err(err)?;}
    let event=json!({"at":now(),"revision":p.revision,"command":crate::project_downloads::redact(cmd),"exitCode":result["exitCode"],"output":crate::project_downloads::redact(result["output"].as_str().unwrap_or("")),"truncated":result["truncated"]});
    let _guard=LOCK.lock().map_err(err)?;
    writeln!(fs::OpenOptions::new().create(true).append(true).open(path).map_err(err)?,"{}",event).map_err(err)
}

/// Metadata for Gutenberg's Markdown verification report (no PDF in that workflow).
pub fn markdown(store:&Store,p:&Project)->String {
    let doc=store.path(&p.id).ok().and_then(|r|fs::symlink_metadata(r.join(".app/model-history.json")).ok().filter(|m|m.file_type().is_file() && m.len()<=2*1024*1024).and_then(|_|fs::read(r.join(".app/model-history.json")).ok())).and_then(|b|serde_json::from_slice::<Value>(&b).ok()).unwrap_or(Value::Null);
    let mut out=String::from("\n\n## Model and conversion duration\n\n");
    let models=doc["models"].as_array();
    if let Some(models)=models.filter(|m|!m.is_empty()) {
        for model in models {out.push_str(&format!("- Model: {}; reasoning effort: {}\n",model["model"].as_str().unwrap_or("not recorded"),model["effort"].as_str().unwrap_or("default")));}
    } else {out.push_str("Model: not recorded for this run.\n");}
    if let Some(start)=doc["startedAt"].as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()) {
        let seconds=(chrono::Utc::now()-start.with_timezone(&chrono::Utc)).num_seconds().max(0);
        out.push_str(&format!("\nTotal elapsed time (including pauses): {}m {}s.\n",seconds/60,seconds%60));
    } else {out.push_str("\nConversion duration: not recorded.\n");}
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn models_keep_history_and_corrupt_optional_metadata_recovers() {
        let dir=tempfile::tempdir().unwrap();let store=Store::open(dir.path().into()).unwrap();let mut p=crate::skill::tests::project();p.phase="running".into();
        store.set(&format!("run-started:{}",p.id),"2026-09-26T10:00:00Z").unwrap();
        selection(&store,&p,"gpt-6-luna","high").unwrap();turn(&store,&p,"t1",false).unwrap();turn(&store,&p,"t1",true).unwrap();selection(&store,&p,"gpt-6-sol","high").unwrap();
        let file=store.path(&p.id).unwrap().join(".app/model-history.json");
        let doc:Value=serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();assert_eq!(doc["models"].as_array().unwrap().len(),2);assert_eq!(doc["startedAt"],"2026-09-26T10:00:00Z");assert!(doc["turns"][0]["endedAt"].is_string());
        let mut corrupt=doc.clone();corrupt["models"]=json!(true);fs::write(&file,corrupt.to_string()).unwrap();selection(&store,&p,"gpt-6-sol","high").unwrap();
        assert!(markdown(&store,&p).contains("gpt-6-sol"));
        let before=fs::read(&file).unwrap();p.phase="deliverable_ready".into();selection(&store,&p,"other-model","high").unwrap();assert_eq!(fs::read(file).unwrap(),before);
    }
}
