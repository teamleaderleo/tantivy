use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use smallvec::smallvec;

use super::{index_documents, AddBatch, AddOperation, IndexWriter, MEMORY_BUDGET_NUM_BYTES_MIN};
use crate::schema::{Schema, STRING};
use crate::{doc, Index, TantivyDocument, TantivyError};

fn retire_repository_workers(index_writer: &mut IndexWriter<TantivyDocument>) -> crate::Result<()> {
    // Close the current document channel and the persistent-worker epoch channels,
    // then join the repository-created workers. The discriminator below installs
    // its own flush receivers so worker ordering is deterministic.
    index_writer.drop_sender();
    index_writer.worker_epoch_senders.clear();
    let original_handles = std::mem::take(&mut index_writer.workers_join_handle);
    for handle in original_handles {
        handle
            .join()
            .expect("repository indexing worker should not panic")?;
    }
    index_writer.worker_flushes.clear();
    assert!(index_writer.index_writer_status.is_alive());
    Ok(())
}

#[test]
fn prepare_commit_error_returns_before_other_epoch_flushes_settle() -> crate::Result<()> {
    let mut schema_builder = Schema::builder();
    let text_field = schema_builder.add_text_field("text", STRING);
    let index = Index::create_in_ram(schema_builder.build());
    let mut index_writer: IndexWriter<TantivyDocument> =
        index.writer_with_num_threads(2, MEMORY_BUDGET_NUM_BYTES_MIN * 2)?;

    retire_repository_workers(&mut index_writer)?;

    // Build one real old-epoch publication path. It cannot run until the test
    // releases it after prepare_commit() has returned.
    let old_opstamp = index_writer.stamper.stamp();
    let old_index = index_writer.index.clone();
    let old_segment_updater = index_writer.segment_updater.clone();
    let old_delete_cursor = index_writer.delete_queue.cursor();
    let old_document = doc!(text_field => "late-old-epoch");
    let (release_old_tx, release_old_rx) = mpsc::channel::<()>();
    let (publication_tx, publication_rx) = mpsc::channel::<Result<(), String>>();
    let (flush_delivery_tx, flush_delivery_rx) = mpsc::channel::<bool>();
    let (late_flush_tx, late_flush_rx) = crossbeam_channel::bounded(1);

    let late_old_worker = thread::spawn(move || {
        release_old_rx
            .recv()
            .expect("test should release the late old-epoch worker");
        let batch: AddBatch<TantivyDocument> = smallvec![AddOperation {
            opstamp: old_opstamp,
            document: old_document,
        }];
        let mut batches = std::iter::once(batch);
        let result = index_documents(
            MEMORY_BUDGET_NUM_BYTES_MIN,
            old_index.new_segment(),
            &mut batches,
            &old_segment_updater,
            old_delete_cursor,
        );
        publication_tx
            .send(result.as_ref().map(|_| ()).map_err(ToString::to_string))
            .expect("test should observe the late publication result");
        flush_delivery_tx
            .send(late_flush_tx.send(result).is_ok())
            .expect("test should observe whether the late flush receiver survived");
    });

    // Model the first persistent worker's failure. A real failed worker sends
    // its Err and returns with its IndexWriterStatus bomb armed, so admission is
    // dead by the time the caller observes the failure. Keep that consequence
    // here so the probe isolates late old-epoch publication rather than new work.
    let (failed_flush_tx, failed_flush_rx) = crossbeam_channel::bounded(1);
    failed_flush_tx
        .send(Err(TantivyError::ErrorInThread(
            "fieldwork synthetic epoch failure".to_string(),
        )))
        .expect("synthetic failed flush should be queued");
    drop(failed_flush_tx);
    drop(index_writer.index_writer_status.create_bomb());
    assert!(!index_writer.index_writer_status.is_alive());

    // The first receiver is already failed; the second receiver has no value and
    // its worker is physically unable to publish until release_old_tx is used.
    index_writer.worker_flushes = vec![failed_flush_rx, late_flush_rx];

    let preparation_error = match index_writer.prepare_commit() {
        Ok(prepared_commit) => {
            prepared_commit.abort()?;
            panic!("synthetic epoch failure should abort prepare_commit")
        }
        Err(error) => error,
    };
    assert!(preparation_error
        .to_string()
        .contains("fieldwork synthetic epoch failure"));

    // This is the discriminator: prepare_commit() has returned even though the
    // second old epoch has neither acknowledged its flush nor published its
    // real segment. Releasing it afterward must still reach SegmentUpdater.
    assert!(publication_rx.try_recv().is_err());
    release_old_tx
        .send(())
        .expect("late old-epoch worker should still be releasable after return");
    publication_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("late old-epoch publication should complete after prepare returned")
        .map_err(TantivyError::ErrorInThread)?;
    assert!(
        !flush_delivery_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("late worker should report flush delivery state"),
        "prepare_commit should have dropped the remaining flush receiver on early return"
    );
    late_old_worker
        .join()
        .expect("late old-epoch worker should not panic");

    Ok(())
}
