use crate::schema::{Field, Schema, STRING};
use crate::{doc, Index, IndexWriter, TantivyDocument, Term};

fn setup() -> crate::Result<(Index, IndexWriter<TantivyDocument>, Field)> {
    let mut schema_builder = Schema::builder();
    let text_field = schema_builder.add_text_field("text", STRING);
    let index = Index::create_in_ram(schema_builder.build());
    let writer = index.writer_for_tests()?;
    Ok((index, writer, text_field))
}

fn commit_and_sync(writer: &mut IndexWriter<TantivyDocument>) -> crate::Result<u64> {
    let opstamp = writer.commit()?;
    // Neutralize the separate committed_opstamp defect tracked upstream in #2666.
    // This probe asks whether delete state is independently hazardous even
    // when the writer's rollback target is kept authoritative.
    writer.committed_opstamp = opstamp;
    assert_eq!(writer.commit_opstamp(), opstamp);
    Ok(opstamp)
}

fn num_docs(index: &Index) -> crate::Result<u64> {
    Ok(index.reader()?.searcher().num_docs())
}

fn push_two_deletes(
    writer: &IndexWriter<TantivyDocument>,
    text_field: Field,
) -> (u64, u64) {
    let first_delete = writer.delete_term(Term::from_field_text(text_field, "hello"));
    let second_delete = writer.delete_term(Term::from_field_text(text_field, "hello"));
    assert!(second_delete > first_delete);
    (first_delete, second_delete)
}

fn force_pending_deletes_into_block(
    writer: &IndexWriter<TantivyDocument>,
    first_delete: u64,
    second_delete: u64,
) {
    // A cursor sitting at the old tail asks for its next block. That forces
    // DeleteQueue's pending writer Vec into an immutable linked block.
    let mut flush_probe = writer.delete_queue.cursor();
    assert_eq!(flush_probe.get().map(|op| op.opstamp), Some(first_delete));
    assert!(flush_probe.advance());
    assert_eq!(flush_probe.get().map(|op| op.opstamp), Some(second_delete));
    drop(flush_probe);
}

#[test]
fn pending_deletes_survive_delete_all_even_with_synced_commit_opstamp() -> crate::Result<()> {
    let (index, mut writer, text_field) = setup()?;

    writer.add_document(doc!(text_field => "hello"))?;
    let first_commit = commit_and_sync(&mut writer)?;
    assert_eq!(num_docs(&index)?, 1);

    let (first_delete, second_delete) = push_two_deletes(&writer, text_field);
    assert!(first_delete > first_commit);

    let rewound_to = writer.delete_all_documents()?;
    assert_eq!(rewound_to, first_commit);
    let _clear_commit = commit_and_sync(&mut writer)?;
    assert_eq!(num_docs(&index)?, 0);

    let readd_opstamp = writer.add_document(doc!(text_field => "hello"))?;
    assert!(
        readd_opstamp <= second_delete,
        "the rewound stamper should reuse an opstamp covered by a pending delete"
    );
    commit_and_sync(&mut writer)?;

    // Characterize the current defect: the stale uncommitted delete is still in
    // DeleteQueue and removes the newly-added document after delete_all_documents().
    assert_eq!(num_docs(&index)?, 0);
    Ok(())
}

#[test]
fn flushed_uncommitted_deletes_cross_delete_all_when_readding_before_commit() -> crate::Result<()> {
    let (index, mut writer, text_field) = setup()?;

    writer.add_document(doc!(text_field => "hello"))?;
    let first_commit = commit_and_sync(&mut writer)?;
    assert_eq!(num_docs(&index)?, 1);

    let (first_delete, second_delete) = push_two_deletes(&writer, text_field);
    assert!(first_delete > first_commit);
    force_pending_deletes_into_block(&writer, first_delete, second_delete);

    // The pending Vec is now empty: the stale deletes live in an immutable
    // queue block. Clear and re-add before the required commit, which is a
    // natural rebuild sequence for delete_all_documents(). The current worker
    // still owns its pre-clear delete cursor during this commit.
    let rewound_to = writer.delete_all_documents()?;
    assert_eq!(rewound_to, first_commit);
    let readd_opstamp = writer.add_document(doc!(text_field => "hello"))?;
    assert!(readd_opstamp <= second_delete);
    commit_and_sync(&mut writer)?;

    // If this remains zero, clearing only DeleteQueue's pending writer Vec at
    // delete_all_documents() cannot be a complete repair: there was nothing
    // left in that Vec at the clear boundary.
    assert_eq!(num_docs(&index)?, 0);
    Ok(())
}

#[test]
fn flushed_uncommitted_deletes_stop_crossing_after_clear_commit() -> crate::Result<()> {
    let (index, mut writer, text_field) = setup()?;

    writer.add_document(doc!(text_field => "hello"))?;
    let first_commit = commit_and_sync(&mut writer)?;
    let (first_delete, second_delete) = push_two_deletes(&writer, text_field);
    force_pending_deletes_into_block(&writer, first_delete, second_delete);

    writer.delete_all_documents()?;
    commit_and_sync(&mut writer)?;
    assert_eq!(num_docs(&index)?, 0);

    let readd_opstamp = writer.add_document(doc!(text_field => "hello"))?;
    assert!(readd_opstamp <= second_delete);
    commit_and_sync(&mut writer)?;

    // The clear commit replaces workers on current main; the replacement
    // delete cursor starts after the already-flushed stale block.
    assert_eq!(num_docs(&index)?, 1);
    assert!(first_delete > first_commit);
    Ok(())
}

#[test]
fn delete_all_without_pending_deletes_allows_readd() -> crate::Result<()> {
    let (index, mut writer, text_field) = setup()?;

    writer.add_document(doc!(text_field => "hello"))?;
    commit_and_sync(&mut writer)?;
    writer.delete_all_documents()?;
    commit_and_sync(&mut writer)?;

    writer.add_document(doc!(text_field => "hello"))?;
    commit_and_sync(&mut writer)?;
    assert_eq!(num_docs(&index)?, 1);
    Ok(())
}

#[test]
fn committed_delete_before_delete_all_does_not_delete_readd() -> crate::Result<()> {
    let (index, mut writer, text_field) = setup()?;

    writer.add_document(doc!(text_field => "hello"))?;
    commit_and_sync(&mut writer)?;
    writer.delete_term(Term::from_field_text(text_field, "hello"));
    commit_and_sync(&mut writer)?;
    assert_eq!(num_docs(&index)?, 0);

    writer.delete_all_documents()?;
    commit_and_sync(&mut writer)?;
    writer.add_document(doc!(text_field => "hello"))?;
    commit_and_sync(&mut writer)?;

    // A delete that was already committed sits below the rewind boundary and
    // should not apply to the new generation.
    assert_eq!(num_docs(&index)?, 1);
    Ok(())
}
