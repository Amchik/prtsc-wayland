use std::time::{SystemTime, UNIX_EPOCH};

#[doc(hidden)]
pub fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("dbg_time: invalid system time configuration")
        .as_millis() as u64
}

#[macro_export]
macro_rules! dbg_time {
    ($($t:tt)*) => {
        {
            let time = $crate::dbg_time::current_time_ms();
            let ret = { $($t)* };
            let time = $crate::dbg_time::current_time_ms() - time;
            if time != 0 {
                println!("Time elapsed: {time}ms");
            }
            ret
        }
    };
}
