#[macro_export]
macro_rules! reexport_strategy {
    ($strategy:ident) => {
        paste::paste! {
            #[doc = concat!("A [Cache] using the [", stringify!($strategy), "Strategy].")]
            pub type [<$strategy Cache>]<K> = $crate::Cache<K, $crate::strategies::$strategy>;
            #[doc = concat!("A [CacheBuilder] using the [", stringify!($strategy), "Strategy].")]
            pub type [<$strategy CacheBuilder>] = $crate::CacheBuilder<$crate::strategies::$strategy>;
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
