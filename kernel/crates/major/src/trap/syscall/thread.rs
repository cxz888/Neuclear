use utils::error::Result;

use crate::task::curr_task;

/// TODO: 写注释
pub fn sys_gettid() -> Result {
    let tid = curr_task().unwrap().inner().res.as_ref().unwrap().tid;
    Ok(tid as isize)
}
