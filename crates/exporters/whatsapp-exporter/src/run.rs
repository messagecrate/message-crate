//! Full export pipeline (wtsexporter, then JSON convert); the desktop app calls
//! it in process.

use crate::emit::{ConvertRequest, convert_json};
use crate::ios_backup::{decrypt_if_encrypted, extract_bytes};
use crate::owner::{owner_from_backup, owner_from_form};
use crate::wtsexporter::{
    Platform, WtsexporterArgs, extracts_ios_backup, resolve_wtsexporter, run_wtsexporter,
};
use anyhow::{Context, Result, bail};
use message_crate_core::{
    ExportTransforms, ExporterConfig, RunResult, ScratchDir, SourceConfig, WHATSAPP_DIRECTORY,
    WhatsappPlatform as CorePlatform, prepare_outputs,
};
use message_staging::{Disk, check_headroom};
use std::env;
use std::fs;

/// Resolve JSON (via wtsexporter or `WhatsappConfig::json`), then convert.
///
/// # Errors
///
/// Returns an error when the source is not WhatsApp, the output directory is or
/// holds what the run reads, wtsexporter cannot run, conversion fails, media
/// processing fails for every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let SourceConfig::Whatsapp(source) = &config.source else {
        bail!("whatsapp-exporter requires SourceConfig::Whatsapp");
    };
    message_crate_core::check_cancel(config.cancel.as_ref())?;
    let mut messages = Vec::new();

    let platform = match source.platform {
        Some(CorePlatform::Android) => Some(Platform::Android),
        Some(CorePlatform::Ios) => Some(Platform::Ios),
        None => None,
    };
    let input = config.primary_input().map(|p| p.to_path_buf());

    // The output is cleaned before it is written, so one that is or holds
    // what this run reads is refused first: the backup (the working directory
    // when none is named and wtsexporter runs) and a ready-made JSON.
    let mut read_paths = config.inputs.clone();
    match &source.json {
        Some(json) => read_paths.push(json.clone()),
        None if input.is_none() => {
            read_paths.push(env::current_dir().context("resolve current working directory")?);
        }
        None => {}
    }
    prepare_outputs(&read_paths, &config.output)?;

    // The number typed on the form, under the server's handle key. Android's
    // only source; iPhone's fallback when the backup carries no owner key.
    let form_owner = source
        .owner_phone
        .as_deref()
        .map(owner_from_form)
        .transpose()?;

    let (json_path, media_roots, owner_identity, _work_keep_alive) = if let Some(json) =
        &source.json
    {
        // Allowed roots are only the backup input and the JSON parent — never
        // the process CWD, which would let crafted paths copy arbitrary files.
        let mut media_roots = Vec::new();
        if let Some(path) = &input {
            media_roots.push(path.clone());
        }
        if let Some(parent) = json.parent() {
            media_roots.push(parent.to_path_buf());
        }
        media_roots.sort();
        media_roots.dedup();
        // A ready-made result.json names no owner; the form's number is all
        // there is, and a conversion may leave it empty.
        (json.clone(), media_roots, form_owner, None)
    } else {
        let platform =
            platform.ok_or_else(|| anyhow::anyhow!("platform is required unless json is set"))?;
        let input = match input {
            Some(path) => path,
            None => env::current_dir().context("resolve current working directory")?,
        };

        message_crate_core::check_cancel(config.cancel.as_ref())?;
        let bin = resolve_wtsexporter()?;
        let work = mark_output_and_make_work_directory(config)?;
        let json_out = work.path().join("result.json");

        // Cooperative only: cancel is checked before and after the external process.
        // Killing wtsexporter mid-run is not implemented.
        message_crate_core::check_cancel(config.cancel.as_ref())?;
        let mut args = WtsexporterArgs {
            platform,
            input: input.clone(),
            work_dir: work.path().to_path_buf(),
            key: source.key.clone(),
            backup: source.backup.clone(),
            wa: source.wa.clone(),
            media: source.media.clone(),
            db: source.db.clone(),
            business: source.business,
        };
        // wtsexporter cannot be given an iPhone backup password, so an
        // encrypted backup's WhatsApp files are decrypted into the work
        // directory first and wtsexporter reads those instead of the backup.
        // From a backup that is not encrypted, wtsexporter extracts them into
        // the work directory itself, so that disk is checked for room first.
        if platform == Platform::Ios {
            if let Some(decrypted) = decrypt_if_encrypted(source, work.path(), config)? {
                args.read_decrypted(decrypted);
            } else if extracts_ios_backup(&args)?
                && let Some(bytes) = extract_bytes(source)?
            {
                check_headroom(work.path(), bytes, Disk::Scratch)?;
            }
        }
        message_crate_core::check_cancel(config.cancel.as_ref())?;
        let log = run_wtsexporter(&bin, &args, &json_out)?;
        message_crate_core::check_cancel(config.cancel.as_ref())?;

        if !log.trim().is_empty() {
            let trimmed = log.trim_end_matches('\n');
            messages.push(trimmed.to_string());
        }

        let kept = config.output.join("wtsexporter_result.json");
        fs::copy(&json_out, &kept).with_context(|| format!("copy JSON to {}", kept.display()))?;

        // Work directory (wtsexporter extract) + backup input. The backup input is the
        // process cwd when the config names no input.
        let mut media_roots = vec![work.path().to_path_buf(), input];
        media_roots.sort();
        media_roots.dedup();

        let owner_identity = match platform {
            // wtsexporter copies the whole app-group domain into the work
            // dir, preferences plist included; a backup someone extracted by
            // hand has it under the input directory. The form's number covers a
            // backup that carries no owner key (the Business app, a moved key).
            Platform::Ios => match owner_from_backup(&media_roots, &mut messages).or(form_owner) {
                Some(owner) => owner,
                None => bail!(
                    "the backup does not contain your WhatsApp phone number; \
                     enter it on the import form"
                ),
            },
            // A crypt backup carries no owner; the form checks the field is
            // filled before the run starts, so this only guards a caller
            // that skipped the form.
            Platform::Android => {
                form_owner.ok_or_else(|| anyhow::anyhow!("Owner's WhatsApp number is required."))?
            }
        };

        (kept, media_roots, Some(owner_identity), Some(work))
    };

    if !json_path.is_file() {
        bail!("JSON not found: {}", json_path.display());
    }

    message_crate_core::check_cancel(config.cancel.as_ref())?;
    let transforms = ExportTransforms::from_config(config);
    let needs_media_tools = transforms.needs_media_tools();
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: &config.output,
        transforms,
        media_search_roots: &media_roots,
        owner_identity,
        output_format: config.output_format,
        cancel: config.cancel.as_ref(),
        resume: config.resume,
        issues: config.issues.as_ref(),
    })?;
    // The work directory goes once the conversion has copied the media.
    drop(_work_keep_alive);

    let mut result = message_crate_core::finish_run(config, &report, needs_media_tools)?;
    messages.append(&mut result.messages);
    result.messages = messages;
    Ok(result)
}

/// Mark the output directory as an export directory, then make the work
/// directory wtsexporter runs in (its working directory, the extract, and
/// `result.json`) under the Scratch Directory's [`WHATSAPP_DIRECTORY`]. The
/// work directory is kept until after convert so media copy can read the
/// extracted files.
///
/// The work directory holds WhatsApp's data in the clear: the decrypted
/// files of an encrypted iPhone backup, and wtsexporter's extract. It is not
/// output, so it is never in the output directory (#1651). It is deleted
/// when the run ends, whichever way it ends; what a killed run left is
/// deleted by the next WhatsApp run and by the sweep when the app starts.
///
/// The writer cleans the output when it opens, and refuses a directory with no
/// sentinel that is not empty, so a new directory must be marked before this
/// run writes into it. It is only marked here, not cleaned: an earlier export
/// in the directory stays until the writer opens, after the new JSON has loaded,
/// so a failed wtsexporter run leaves it in place. A resumed run's directory is
/// already marked.
///
/// # Errors
///
/// Returns an error when the output cannot be read or marked, holds files and
/// no sentinel, or the work directory cannot be made.
fn mark_output_and_make_work_directory(config: &ExporterConfig) -> Result<ScratchDir> {
    if !config.resume {
        message_ir_format::mark_export_directory(&config.output)?;
    }
    ScratchDir::create(&config.scratch_dir.join(WHATSAPP_DIRECTORY))
}

#[cfg(test)]
mod tests {
    use message_crate_core::testutil::jsonl_run_config;
    use message_crate_core::{ExporterConfig, SourceConfig, WhatsappConfig};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// The names in `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The writer cleans its output directory before it writes, so an output
    /// that is or holds the backup or the JSON would delete what is being
    /// read.
    #[test]
    fn run_refuses_an_output_that_is_or_contains_an_input() {
        let tmp = tempfile::tempdir().unwrap();
        let backup = tmp.path().join("backup");
        fs::create_dir_all(&backup).unwrap();
        let json = backup.join("result.json");
        fs::write(&json, "{}").unwrap();

        for (inputs, output) in [
            // The JSON's directory.
            (vec![], backup.clone()),
            // A directory above the backup named as the input.
            (vec![backup.as_path()], tmp.path().to_path_buf()),
        ] {
            let config = jsonl_run_config(
                &inputs,
                &output,
                SourceConfig::Whatsapp(WhatsappConfig {
                    json: Some(json.clone()),
                    ..WhatsappConfig::default()
                }),
            );
            let before = entries(&output);
            let err = crate::run(&config).unwrap_err().to_string();
            assert!(
                err.contains("must not be the same as, or contain, the input"),
                "{}: {err}",
                output.display()
            );
            assert!(json.is_file(), "the JSON is left as it was");
            assert_eq!(
                entries(&output),
                before,
                "nothing is written to {}",
                output.display()
            );
        }
    }

    /// An empty output directory `out` in a new temporary directory, and a
    /// WhatsApp run's config that writes into it, with its Scratch Directory
    /// at `scratch` beside it.
    fn empty_whatsapp_output() -> (tempfile::TempDir, PathBuf, ExporterConfig) {
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("out");
        fs::create_dir_all(&output).unwrap();
        let mut config = jsonl_run_config(
            &[],
            &output,
            SourceConfig::Whatsapp(WhatsappConfig::default()),
        );
        config.scratch_dir = tmp.path().join("scratch");
        (tmp, output, config)
    }

    /// The work directory, which holds WhatsApp's data in the clear, is made
    /// under the Scratch Directory's WhatsApp directory, where the sweep
    /// finds what a killed run left, and never in the output directory. The
    /// output is marked first, so wtsexporter's JSON copied there is
    /// accepted by the writer's clean. The work directory is gone when the
    /// run lets go of it (#1651).
    #[test]
    fn the_work_directory_is_made_under_the_scratch_directory_not_the_output() {
        let (_tmp, output, config) = empty_whatsapp_output();

        let work = super::mark_output_and_make_work_directory(&config).unwrap();

        let root = config
            .scratch_dir
            .join(message_crate_core::WHATSAPP_DIRECTORY);
        assert_eq!(work.path().parent(), Some(root.as_path()));
        assert_eq!(
            entries(&output),
            [message_ir_format::EXPORT_SENTINEL.to_string()],
            "only the mark is in the output"
        );
        fs::write(output.join("wtsexporter_result.json"), "{}").unwrap();
        message_ir_format::clean_previous_ir_output(&output).unwrap();

        let path = work.path().to_path_buf();
        drop(work);
        assert!(!path.exists(), "the work directory is deleted");
    }

    /// Making the work directory leaves an earlier export in place, so a
    /// wtsexporter run that fails afterwards has not deleted it.
    #[test]
    fn making_the_work_directory_keeps_an_earlier_export() {
        let (_tmp, output, config) = empty_whatsapp_output();
        message_ir_format::mark_export_directory(&output).unwrap();
        fs::write(output.join("earlier.jsonl"), "{}").unwrap();

        let _work = super::mark_output_and_make_work_directory(&config).unwrap();

        assert!(output.join("earlier.jsonl").is_file());
    }
}
