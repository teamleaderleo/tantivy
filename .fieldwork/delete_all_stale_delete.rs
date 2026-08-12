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
    // This probe asks whether pending deletes are independently hazardous even
    // when the writer's rollback target is kept authoritative.
    writer.committed_opstamp = opstamp;
    assert_eq!(writer.commit_opstamp(), opstamp);
    Ok(opstamp)
}

fn num_docs(index: &Index) -> crate::Result<u64> {
    Ok(index.reader()?.searcher().num_docs())
}

#[test]
fn pending_deletes_survive_delete_all_even_with_synced_commit_opstamp() -> crate::Result<()> {
    let (index, mut writer, text_field) = setup()?;

    writer.add_document(doc!(text_field => "hello"))?;
    let first_commit = commit_and_sync(&mut writer)?;
    assert_eq!(num_docs(&index)?, 1);

    let first_delete = writer.delete_term(Term::from_field_text(text_field, "hello"));
    let second_delete = writer.delete_term(Term::from_field_text(text_field, "hello"));
    assert!(first_delete > first_commit);
    assert!(second_delete > first_delete);

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
