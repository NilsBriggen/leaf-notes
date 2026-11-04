//! Android: calls into LeafActivity (android/java) for the file picker, files shared into the app,
//! opening attachments, the key store and the system language.
use crate::audio::with_activity;
use jni::{objects::{JObject, JString}, strings::JNIString, Env};
use std::path::{Path, PathBuf};

fn string(env: &mut Env<'_>, o: JObject<'_>) -> jni::errors::Result<String> {
    let s = env.cast_local::<JString>(o)?;
    s.try_to_string(env)
}

/// Show the system file picker; the result arrives through `incoming` with this kind.
pub fn pick(kind: i32, mime: &str, many: bool) -> Result<(), String> {
    with_activity(|env, act| {
        let m = env.new_string(mime)?;
        env.call_method(&act, JNIString::new("pickFile"), jni::jni_sig!("(ILjava/lang/String;Z)V"), &[kind.into(), (&m).into(), many.into()])?;
        Ok(())
    })
}

/// Files picked or shared since the last call: (kind, where the copy is).
pub fn incoming() -> Vec<(i32, PathBuf)> {
    let text = with_activity(|env, act| {
        let o = env.call_method(&act, JNIString::new("takeIncoming"), jni::jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
        string(env, o)
    }).unwrap_or_default();
    text.lines().filter_map(|l| l.split_once('|')).filter_map(|(k, p)| Some((k.parse().ok()?, PathBuf::from(p)))).collect()
}

/// Something was picked or shared and waits to be collected.
pub fn has_incoming() -> bool {
    with_activity(|env, act| env.call_method(&act, JNIString::new("hasIncoming"), jni::jni_sig!("()Z"), &[])?.z()).unwrap_or(false)
}

/// Open a file in another app (via a copy in Downloads/Leaf Notes).
pub fn open(path: &Path, name: &str) {
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "pdf" => "application/pdf", "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "txt" | "md" => "text/plain", "wav" => "audio/wav",
        "doc" | "docx" => "application/msword", "zip" => "application/zip", _ => "application/octet-stream",
    };
    let _ = with_activity(|env, act| {
        let (p, n, m) = (env.new_string(path.to_string_lossy())?, env.new_string(name)?, env.new_string(mime)?);
        env.call_method(&act, JNIString::new("openFile"), jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"), &[(&p).into(), (&n).into(), (&m).into()])?;
        Ok(())
    });
}

/// The system language ("de", "fr", …).
pub fn language() -> Option<String> {
    with_activity(|env, act| {
        let o = env.call_method(&act, JNIString::new("language"), jni::jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
        string(env, o)
    }).ok()
}

fn crypt(method: &str, input: &str) -> Option<String> {
    with_activity(|env, act| {
        let s = env.new_string(input)?;
        let o = env.call_method(&act, JNIString::new(method), jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"), &[(&s).into()])?.l()?;
        string(env, o)
    }).ok().filter(|r| !r.is_empty())
}

/// Encrypt with a key that lives in the Android key store (None: not available).
pub fn protect(plain: &str) -> Option<String> { crypt("protect", plain) }
pub fn unprotect(sealed: &str) -> Option<String> { crypt("unprotect", sealed) }
