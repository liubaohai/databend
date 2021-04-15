// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_transform_partial_aggregator() -> anyhow::Result<()> {
    use std::sync::Arc;

    use common_planners::*;
    use common_planners::{self};
    use futures::TryStreamExt;
    use pretty_assertions::assert_eq;

    use crate::pipelines::processors::*;
    use crate::pipelines::transforms::*;

    let ctx = crate::tests::try_create_context()?;
    let test_source = crate::tests::NumberTestData::create(ctx.clone());

    // sum(number)+1, avg(number)
    let aggr_exprs = vec![add(sum(col("number")), lit(2u64)), avg(col("number"))];
    let aggr_partial = PlanBuilder::create(test_source.number_schema_for_test()?)
        .aggregate_partial(aggr_exprs.clone(), vec![])?
        .build()?;

    // Pipeline.
    let mut pipeline = Pipeline::create();
    let source = test_source.number_source_transform_for_test(200000)?;
    pipeline.add_source(Arc::new(source))?;
    pipeline.add_simple_transform(|| {
        Ok(Box::new(AggregatorPartialTransform::try_create(
            aggr_partial.schema(),
            aggr_exprs.clone(),
        )?))
    })?;
    pipeline.merge_processor()?;

    // Result.
    let stream = pipeline.execute().await?;
    let result = stream.try_collect::<Vec<_>>().await?;
    let block = &result[0];
    assert_eq!(block.num_columns(), 2);

    let expected = vec![
        "+--------------------------------------------------+--------------------------------------------------------------------+",
        "| plus(sum(number), 2)                             | avg(number)                                                        |",
        "+--------------------------------------------------+--------------------------------------------------------------------+",
        "| {\"Struct\":[{\"UInt64\":19999900000},{\"UInt64\":2}]} | {\"Struct\":[{\"Struct\":[{\"UInt64\":19999900000},{\"UInt64\":200000}]}]} |",
        "+--------------------------------------------------+--------------------------------------------------------------------+",
    ];
    crate::assert_blocks_sorted_eq!(expected, result.as_slice());

    Ok(())
}
