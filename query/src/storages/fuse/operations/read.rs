//  Copyright 2021 Datafuse Labs.
//
//  Licensed under the Apache License, Version 2.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
//

use std::sync::Arc;

use common_datavalues::DataSchema;
use common_exception::ErrorCode;
use common_exception::Result;
use common_planners::Extras;
use common_streams::ParquetSource;
use common_streams::SendableDataBlockStream;
use common_streams::Source;
use common_tracing::tracing_futures::Instrument;
use futures::StreamExt;

use super::part_info::PartInfo;
use crate::sessions::QueryContext;
use crate::storages::fuse::FuseTable;

impl FuseTable {
    #[inline]
    pub async fn do_read(
        &self,
        ctx: Arc<QueryContext>,
        push_downs: &Option<Extras>,
    ) -> Result<SendableDataBlockStream> {
        let projection = if let Some(Extras {
            projection: Some(prj),
            ..
        }) = push_downs
        {
            prj.clone()
        } else {
            (0..self.table_info.schema().fields().len())
                .into_iter()
                .collect::<Vec<usize>>()
        };

        let bite_size = ctx.get_settings().get_parallel_read_threads()?;
        let ctx_clone = ctx.clone();
        let iter =
            std::iter::from_fn(
                move || match ctx_clone.clone().try_get_partitions(bite_size) {
                    Err(_) => None,
                    Ok(parts) if parts.is_empty() => None,
                    Ok(parts) => Some(parts),
                },
            )
            .flatten();
        let da = ctx.get_data_accessor()?;
        let arrow_schema = self.table_info.schema().to_arrow();
        let table_schema = Arc::new(DataSchema::from(arrow_schema));

        let part_stream = futures::stream::iter(iter);

        let read_buffer_size = ctx.get_settings().get_storage_read_buffer_size()?;
        let stream = part_stream
            .map(move |part| {
                let da = da.clone();
                let table_schema = table_schema.clone();
                let projection = projection.clone();
                async move {
                    let part_info = PartInfo::decode(&part.name)?;
                    let part_location = part_info.location();
                    let part_len = part_info.length();

                    let mut source = ParquetSource::with_hints(
                        da,
                        part_info.location().to_owned(),
                        table_schema,
                        projection,
                        None, // TODO cache parquet meta
                        Some(part_len),
                        Some(read_buffer_size),
                    );
                    source
                        .read()
                        .await
                        .map_err(|e| {
                            ErrorCode::ParquetError(format!(
                                "fail to read block {}, {}",
                                part_location, e
                            ))
                        })?
                        .ok_or_else(|| {
                            ErrorCode::ParquetError(format!(
                                "reader returns None for block {}",
                                part_location,
                            ))
                        })
                }
            })
            .buffer_unordered(bite_size as usize)
            .instrument(common_tracing::tracing::Span::current());
        Ok(Box::pin(stream))
    }
}
