use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    jobs::{self, JobEvent, JobRecord, JobState},
    models::AfterProcessing,
};

pub async fn complete(
    workspace: &Path,
    record: &JobRecord,
    behavior: &AfterProcessing,
) -> Result<JobRecord, String> {
    let workspace = workspace.to_path_buf();
    let id = record.id.clone();
    let expected_run = record.active_run.clone();
    let behavior = behavior.clone();
    tokio::task::spawn_blocking(move || {
        let record = jobs::load(&workspace, &id)?;
        if record.active_run != expected_run
            || record.state != JobState::Archiving
            || !record.remote_confirmed
            || record.task_commit.is_none()
        {
            return Err("only a verified publication can archive its source PDF".into());
        }
        let destination = match record.archive_destination.clone() {
            Some(destination) => Some(destination),
            None => planned_archive_destination(&record.source_path, &workspace, &id, &behavior)?,
        };
        jobs::apply_event(
            &workspace,
            &record,
            JobEvent::ArchivePlanned(destination.clone()),
        )?;
        let source = if record.source_path.is_file()
            && jobs::sha256(&record.source_path)? == record.sha256
        {
            &record.source_path
        } else {
            &record.input_path
        };
        archive_job_source(
            source,
            &record.input_path,
            destination.as_deref(),
            &behavior,
        )?;
        jobs::apply_event(&workspace, &record, JobEvent::Archived)
    })
    .await
    .map_err(|error| format!("join source archival: {error}"))?
}

fn planned_archive_destination(
    source: &Path,
    workspace: &Path,
    job_id: &str,
    behavior: &AfterProcessing,
) -> Result<Option<PathBuf>, String> {
    if matches!(behavior, AfterProcessing::Keep) {
        return Ok(None);
    }
    let filename = source
        .file_name()
        .ok_or_else(|| "source PDF has no filename".to_string())?;
    let destination_root = match behavior {
        AfterProcessing::MoveToDone => workspace.join("done"),
        AfterProcessing::Trash => {
            PathBuf::from(std::env::var("HOME").map_err(|_| "HOME is unavailable".to_string())?)
                .join(".Trash")
        }
        AfterProcessing::Keep => unreachable!(),
    };
    fs::create_dir_all(&destination_root)
        .map_err(|error| format!("create archive directory: {error}"))?;
    let mut destination = destination_root.join(filename);
    if destination.exists() {
        destination = destination_root.join(format!("{job_id}-{}", filename.to_string_lossy()));
    }
    Ok(Some(destination))
}

fn archive_source(
    source: &Path,
    destination: Option<&Path>,
    behavior: &AfterProcessing,
) -> Result<(), String> {
    if matches!(behavior, AfterProcessing::Keep) {
        return Ok(());
    }
    let destination =
        destination.ok_or_else(|| "archive destination is unavailable".to_string())?;
    if !source.exists() {
        return if destination.exists() {
            Ok(())
        } else {
            Err("source PDF and planned archive destination are both missing".into())
        };
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(18) => {
            let mut input = fs::File::open(source)
                .map_err(|copy_error| format!("open source PDF for archival: {copy_error}"))?;
            let temporary = destination.with_extension("cognitio-copying");
            let mut output = fs::File::create(&temporary)
                .map_err(|copy_error| format!("create archive PDF: {copy_error}"))?;
            std::io::copy(&mut input, &mut output)
                .map_err(|copy_error| format!("copy source PDF to archive: {copy_error}"))?;
            output
                .sync_all()
                .map_err(|copy_error| format!("sync archived PDF: {copy_error}"))?;
            fs::rename(&temporary, destination)
                .map_err(|copy_error| format!("finish archived PDF: {copy_error}"))?;
            fs::remove_file(source)
                .map_err(|copy_error| format!("remove archived source PDF: {copy_error}"))
        }
        Err(error) => Err(format!("archive source PDF: {error}")),
    }
}

fn archive_job_source(
    source: &Path,
    input: &Path,
    destination: Option<&Path>,
    behavior: &AfterProcessing,
) -> Result<(), String> {
    let archival_source = if source.exists() || !input.exists() {
        source
    } else {
        input
    };
    archive_source(archival_source, destination, behavior)?;
    if input != archival_source && input.exists() {
        fs::remove_file(input).map_err(|error| format!("remove processing PDF: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn archival_preserves_replaced_source_and_resumes_after_files_were_moved() {
        for interrupted in [false, true] {
            let root = std::env::temp_dir()
                .join(format!("cognitio-archive-resume-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("inbox")).unwrap();
            fs::create_dir_all(root.join("done")).unwrap();
            let source = root.join("inbox/paper.pdf");
            fs::write(&source, b"original PDF").unwrap();
            let mut record =
                jobs::create(&root, &source, &crate::models::AppSettings::default()).unwrap();
            record.state = JobState::Archiving;
            record.stage = jobs::JobStage::Archiving;
            record.remote_confirmed = true;
            record.task_commit = Some("verified-task-commit".into());
            let destination = root.join("done/paper.pdf");
            record.archive_destination = Some(destination.clone());
            jobs::save(&root, &record).unwrap();
            fs::write(&source, b"replacement PDF").unwrap();
            if interrupted {
                fs::rename(&record.input_path, &destination).unwrap();
            }
            let completed = complete(&root, &record, &AfterProcessing::MoveToDone)
                .await
                .unwrap();
            assert_eq!(completed.state, JobState::Succeeded);
            assert_eq!(fs::read(&source).unwrap(), b"replacement PDF");
            assert_eq!(fs::read(&destination).unwrap(), b"original PDF");
            assert!(!record.input_path.exists());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn archival_falls_back_to_the_processing_copy() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-archive-fallback-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let source = root.join("inbox/paper.pdf");
        let input = root.join("processing/job/paper.pdf");
        let destination = root.join("done/paper.pdf");
        fs::create_dir_all(input.parent().expect("input parent")).expect("processing directory");
        fs::create_dir_all(destination.parent().expect("destination parent"))
            .expect("done directory");
        fs::write(&input, b"%PDF-1.4\n").expect("processing PDF");

        archive_job_source(
            &source,
            &input,
            Some(&destination),
            &AfterProcessing::MoveToDone,
        )
        .expect("archive processing copy");

        assert!(!input.exists());
        assert_eq!(fs::read(&destination).expect("archived PDF"), b"%PDF-1.4\n");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn blocked_publication_cannot_touch_source_or_processing_copy() {
        let root =
            std::env::temp_dir().join(format!("cognitio-archive-block-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        fs::write(&source, b"%PDF-1.4\n").unwrap();
        let record = jobs::create(&root, &source, &crate::models::AppSettings::default()).unwrap();
        jobs::update(&root, &record.id, JobState::Preflight, "preflight", 15).unwrap();
        jobs::block(&root, &record.id, "push failed", jobs::BlockScope::Job).unwrap();

        assert!(complete(&root, &record, &AfterProcessing::MoveToDone)
            .await
            .is_err());
        assert!(source.is_file());
        assert!(record.input_path.is_file());
        assert!(!root.join("done").exists());
        assert_eq!(
            jobs::load(&root, &record.id).unwrap().state,
            JobState::Blocked
        );
        fs::remove_dir_all(root).unwrap();
    }
}
