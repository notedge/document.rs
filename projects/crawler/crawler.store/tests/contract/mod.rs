//! Y-1..Y-8 store contract suite shared by `MemoryStore` and `YydbStore`.

#[cfg(feature = "yydb")]
mod g4_doctor_quota_lag;
mod support;
mod y1_batch_transaction;
mod y2_single_winner;
mod y3_lease_fencing;
mod y4_scan_paging;
mod y5_ttl_reopen;
mod y6_objects;
mod y7_checkpoint_reopen;
#[cfg(feature = "yydb")]
mod y8_udf;
