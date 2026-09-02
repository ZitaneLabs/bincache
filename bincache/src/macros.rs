macro_rules! reexport_strategy {
    ($strategy:ident, $cache:ident, $builder:ident, $strategy_alias:ident) => {
        #[doc = concat!("A [Cache] using the [", stringify!($strategy), "Strategy].")]
        pub type $cache<K, C> = $crate::Cache<K, $crate::strategies::$strategy, C>;
        #[doc = concat!("A [CacheBuilder] using the [", stringify!($strategy), "Strategy].")]
        pub type $builder =
            $crate::cache_builder::CacheBuilderWithStrategy<$crate::strategies::$strategy>;
        pub use $crate::strategies::$strategy as $strategy_alias;

        const _: () = {
            fn assert_default<T: Default>() {}
            fn assert_strategy<T: $crate::traits::CacheStrategy>() {}

            fn assert_all() {
                assert_default::<$crate::strategies::$strategy>();
                assert_strategy::<$crate::strategies::$strategy>();
            }
        };
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
