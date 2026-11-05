//! Proxy lifecycle commands; originals remain the authoritative media references.
use super::kit::*;

fn flag(a: &Value, key: &str, default: bool) -> Result<bool, String> {
    match a.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("{key} must be a boolean")),
    }
}
fn rows(ss: &Session, deep: bool) -> Vec<Value> {
    ss.project
        .proxies
        .entries
        .keys()
        .map(|id| crate::proxy::status(&ss.project, id, deep))
        .collect()
}
pub fn commands() -> Vec<Command> {
    vec![
        Command { name:"proxy_generate", description:"Generate and register a disposable, intra-frame H.264 MP4 preview proxy for one imported video asset. Validates every source/output timestamp, frame count, rational rate, aspect ratio and SHA256. Opaque 8-bit SDR, square pixels, zero-start CFR only (1..240 fps, at most 216000 frames). Never overwrites files; parent directory must exist. Cancellable with bounded timeout. One undo step detaches the proxy but leaves its file. Enable with proxy_set_enabled. Exports, audio and analysis always use originals.", mutates:true,
            schema:obj(vec![("asset_id",s("imported video asset")),("path",s("new .mp4 output; existing parent directory")),("max_side",json!({"type":"integer","minimum":64,"maximum":4096,"description":"maximum proxy side; default 960; exact source aspect preserved"})),("timeout_seconds",json!({"type":"integer","minimum":1,"maximum":3600,"description":"subprocess deadline budget; default 300 seconds"}))],&["asset_id","path"]),
            run:|ss,a| { let id = str_arg(a,"asset_id")?.to_string(); let bound = a.get("max_side").map(|v| v.as_u64().ok_or("max_side must be a positive integer")).transpose()?.unwrap_or(960);
                let seconds = a.get("timeout_seconds").map(|v| v.as_u64().ok_or("timeout_seconds must be an integer")).transpose()?.unwrap_or(300);
                if !(1..=3600).contains(&seconds) || bound>4096 { return Err("invalid proxy bounds".into()); }
                let q = crate::proxy::generate(&ss.project,&id,str_arg(a,"path")?,bound as u32,std::time::Duration::from_secs(seconds.min(crate::guard::limits().export_timeout_s)))?;
                ss.project.proxies.entries.insert(id.clone(),q);
                Ok(json!({"proxy":crate::proxy::status(&ss.project,&id,false),"originals_on_export":true}))
            } },
        Command { name:"proxy_list", description:"List registered preview proxies with missing/changed source or proxy states, preview eligibility, dimensions, frame rate, counts and sizes. Fast size/mtime checks; proxy_validate performs uncached SHA256 checks. Read-only.", mutates:false,
            schema:obj(vec![],&[]),run:|ss,_| Ok(json!({"enabled":ss.project.proxies.enabled,"proxies":rows(ss,false),"originals_on_export":true,"audio_from_originals":true})) },
        Command { name:"proxy_set_enabled", description:"Enable/disable proxy use in render_frame PNG and preview_range. Undoable project preference; unavailable/stale proxies fall back to originals. Managed-color projects bypass proxies. All export, audio, tracking and analysis paths keep originals.", mutates:true,
            schema:obj(vec![("enabled",json!({"type":"boolean","description":"use eligible proxies in editing previews"}))],&["enabled"]),run:|ss,a| { let enabled=a.get("enabled").and_then(Value::as_bool).ok_or("enabled must be a boolean")?; ss.project.proxies.enabled=enabled; Ok(json!({"enabled":enabled,"proxies":rows(ss,false)})) } },
        Command { name:"proxy_remove", description:"Detach one proxy from the project in one undoable step. Keeps the proxy file on disk and never changes/deletes original media. Undo can restore the binding.", mutates:true,
            schema:obj(vec![("asset_id",s("asset whose proxy should be detached"))],&["asset_id"]),run:|ss,a| { let id=str_arg(a,"asset_id")?; let q=ss.project.proxies.entries.remove(id).ok_or("no registered proxy for asset")?; Ok(json!({"asset_id":id,"detached":true,"retained_file":q.path})) } },
        Command { name:"proxy_validate", description:"Validate all registered proxies or one asset. Deep (default true) performs uncached SHA256 source/proxy verification, including changes preserving size and mtime; false uses fast stat fingerprints. Read-only. Identical relocated copies can be rebound using proxy_relink.", mutates:false,
            schema:obj(vec![("asset_id",s("optional single asset")),("deep",json!({"type":"boolean","description":"full uncached hashes; default true"}))],&[]),run:|ss,a| { let deep=flag(a,"deep",true)?; let result=if a.get("asset_id").is_some() { vec![crate::proxy::status(&ss.project,str_arg(a,"asset_id")?,deep)] } else { rows(ss,deep) }; Ok(json!({"deep":deep,"proxies":result})) } },
        Command { name:"proxy_relink", description:"Recover a missing/moved proxy by binding a byte-identical copy. Rehashes both original and replacement against the recorded SHA256; rejects different media. Refreshes fingerprints after relocation/collection. Undoable; no files written/deleted.", mutates:true,
            schema:obj(vec![("asset_id",s("asset with a registered proxy")),("path",s("existing byte-identical proxy copy"))],&["asset_id","path"]),run:|ss,a| {
                let id=str_arg(a,"asset_id")?.to_string(); let p=&ss.project;
                let mut q=p.proxies.entries.get(&id).ok_or("no registered proxy for asset")?.clone();
                let asset=p.assets.get(&id).ok_or("no such asset")?;
                let source=crate::proxy::fingerprint(std::path::Path::new(&p.resolve(&asset.path)))?;
                let path=crate::deps::plain_path(std::path::PathBuf::from(p.resolve(str_arg(a,"path")?)).canonicalize().map_err(|e| e.to_string())?);
                if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4")) { return Err("proxy replacement must retain the .mp4 extension".into()); } let proxy=crate::proxy::fingerprint(&path)?;
                if source.sha256!=q.source.sha256 || proxy.sha256!=q.proxy.sha256 { return Err("proxy_relink requires byte-identical source and proxy media; regenerate for changed footage".into()); }
                q.source=source; q.proxy=proxy; q.path=path.to_string_lossy().into_owned(); ss.project.proxies.entries.insert(id.clone(),q);
                Ok(json!({"proxy":crate::proxy::status(&ss.project,&id,false)}))
            } },
    ]
}
