macro_rules! reexport_strategy {
    ($strategy:ident) => {
        paste::paste! {
            #[doc = concat!("A [Cache] using the [", stringify!($strategy), "Strategy].")]
            pub type [<$strategy Cache>]<K, C> = $crate::Cache<K, $crate::strategies::$strategy, C>;
            #[doc = concat!("A [CacheBuilder] using the [", stringify!($strategy), "Strategy].")]
            pub type [<$strategy CacheBuilder>] = $crate::cache_builder::CacheBuilderWithStrategy<$crate::strategies::$strategy>;
            pub use $crate::strategies::$strategy as [<$strategy Strategy>];

            const _: () = {
                fn assert_default<T: Default>() {}
                fn assert_strategy<T: $crate::traits::CacheStrategy>() {}

                fn assert_all() {
                    assert_default::<$crate::strategies::$strategy>();
                    assert_strategy::<$crate::strategies::$strategy>();
                }
            };
        }
    };
}

pub(crate) use reexport_strategy;

// We wanna be able to use the right async runtime for the right feature,
// but we also want to be able to use the same code for all of them.
#[cfg(test)]
#[macro_export]
macro_rules! async_test {
    ($(async fn $name:ident () $body:block)+) => {
        $(
            #[cfg_attr(any(
                feature = "blocking",
                feature = "rt_tokio_1",
                all(feature = "implicit-blocking", not(feature = "rt_async-std_1")),
            ), tokio::test(flavor = "multi_thread"))]
            #[cfg_attr(feature = "rt_async-std_1", async_std::test)]
            async fn $name () $body
        )+
    };
}
