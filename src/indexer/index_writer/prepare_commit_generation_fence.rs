use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use smallvec::smallvec;

use super::{
    index_documents, AddBatch, AddOperation, IndexWriter, MEMORY_BUDGET_NUM_BYTES_MIN,
};
use crate::schema::{Schema, STRING};
use crate::{doc, Index, TantivyDocument, TantivyError, Term};

#[test]
fn prepare_commit_failure_leaves_next_generation_live_and_accepts_late_old_segment(
) -> crate::Result<()> {
    let mut schema_builder = Schema::builder();
    let text_field = schema_builder.add_text_field("text", STRING);
    let index = Index::create_in_ram(schema_builder.build());
    let mut index_writer: IndexWriter<TantivyDocument> =
        index.writer_with_num_threads(3, MEMORY_BUDGET_NUM_BYTES_MIN * 3)?;

    // Retire the repository-created workers so the join order below is fully
    // deterministic. This leaves a fresh, live document channel with no workers.
    index_writer.recreate_document_channel();
    let original_handles = std::mem::take(&mut index_writer.workers_join_handle);
    for handle in original_handles {
        handle
            .join()
            .expect("repository indexing worker should not panic")?;
    }
    assert!(index_writer.workers_join_handle.is_empty());

    // Model one old-generation worker that has already taken ownership of a
    // document but has not yet published its segment. Its JoinHandle will be the
    // third entry and therefore dropped when the second synthetic worker fails.
    let old_opstamp = index_writer.stamper.stamp();
    let old_index = index_writer.index.clone();
    let old_segment_updater = index_writer.segment_updater.clone();
    let old_delete_cursor = index_writer.delete_queue.cursor();
    let old_document = doc!(text_field => "old-generation-late");
    let (release_old_tx, release_old_rx) = mpsc::channel::<()>();
    let (publication_tx, publication_rx) = mpsc::channel::<Result<(), String>>();
    let late_old_worker = thread::spawn(move || {
        release_old_rx
            .recv()
            .expect("test should release the detached old worker");
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
            .expect("test should observe old-worker publication");
        result
    });

    let successful_worker = thread::spawn(|| Ok(()));
    let failed_worker = thread::spawn(|| {
        Err(TantivyError::ErrorInThread(
            "fieldwork synthetic old-worker failure".to_string(),
        ))
    });
    index_writer.workers_join_handle = vec![successful_worker, failed_worker, late_old_worker];

    let preparation_error = match index_writer.prepare_commit() {
        Ok(prepared_commit) => {
            prepared_commit.abort()?;
            panic!("synthetic old-worker failure should abort prepare_commit")
        }
        Err(error) => error,
    };
    assert!(
        preparation_error
            .to_string()
            .contains("fieldwork synthetic old-worker failure")
    );

    // The first successful join already spawned one replacement on the new
    // document channel. The later failure did not retire that generation.
    assert_eq!(index_writer.workers_join_handle.len(), 1);
    assert!(index_writer.index_writer_status.is_alive());
    index_writer.add_document(doc!(text_field => "new-generation"))?;

    // The third old worker was not joined. Dropping its JoinHandle did not stop
    // it, and it can still publish through the shared SegmentUpdater.
    assert!(publication_rx.try_recv().is_err());
    release_old_tx
        .send(())
        .expect("detached old worker should still be running");
    publication_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("detached old worker should publish after prepare_commit returns")
        .map_err(TantivyError::ErrorInThread)?;

    // A later commit accepts both the newly admitted document and the segment
    // published by the detached old generation.
    index_writer.commit()?;
    let searcher = index.reader()?.searcher();
    let old_term = Term::from_field_text(text_field, "old-generation-late");
    let new_term = Term::from_field_text(text_field, "new-generation");
    assert_eq!(searcher.doc_freq(&old_term)?, 1);
    assert_eq!(searcher.doc_freq(&new_term)?, 1);

    Ok(())
}
