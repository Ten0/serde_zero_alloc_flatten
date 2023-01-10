#[derive(serde_derive::Deserialize, PartialEq, Debug)]
#[allow(unused)]
struct A {
	a: i32,
	#[serde(flatten)]
	b: B,
}
#[derive(serde_derive::Deserialize, PartialEq, Debug)]
#[allow(unused)]
struct B {
	d: usize,
}

const FLATTEN_JSON: &str = include_str!("../benches/flatten.json");
#[test]
fn zero_alloc_flatten() {
	let res = zero_alloc_deserialize_for_a::deserialize(&mut serde_json::Deserializer::from_str(FLATTEN_JSON)).unwrap();
	assert_eq!(A { a: 3, b: B { d: 12 } }, res);
}

mod zero_alloc_deserialize_for_a {
	use super::{A, B};

	pub(crate) fn deserialize<'de, D>(__deserializer: D) -> Result<A, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		#[allow(non_camel_case_types)]
		enum __Field {
			__field0,
			__forward_to_flattened,
		}
		struct __FieldVisitor;

		impl<'de> serde::de::Visitor<'de> for __FieldVisitor {
			type Value = __Field;
			fn expecting(&self, __formatter: &mut serde::__private::Formatter) -> serde::__private::fmt::Result {
				serde::__private::Formatter::write_str(__formatter, "field identifier")
			}

			fn visit_str<__E>(self, __value: &str) -> Result<Self::Value, __E>
			where
				__E: serde::de::Error,
			{
				match __value {
					"a" => serde::__private::Ok(__Field::__field0),
					_ => serde::__private::Ok(__Field::__forward_to_flattened),
				}
			}
			fn visit_bytes<__E>(self, __value: &[u8]) -> Result<Self::Value, __E>
			where
				__E: serde::de::Error,
			{
				match __value {
					b"a" => serde::__private::Ok(__Field::__field0),
					_ => serde::__private::Ok(__Field::__forward_to_flattened),
				}
			}
		}
		impl<'de> serde::Deserialize<'de> for __Field {
			#[inline]
			fn deserialize<__D>(__deserializer: __D) -> Result<Self, __D::Error>
			where
				__D: serde::Deserializer<'de>,
			{
				serde::Deserializer::deserialize_identifier(__deserializer, __FieldVisitor)
			}
		}

		struct __FlattenSpec {
			field0: serde::__private::Option<i32>,
		}

		// Todo check to what extent stuff should be #[inline]
		impl<'de> serde_zero_alloc_flatten::FlattenSpec<'de> for __FlattenSpec {
			type Key = __Field;
			fn should_forward_to_flatten_field(key: &Self::Key) -> bool {
				// alternately derive PartialEq on field - not sure whether that has any impact
				matches!(key, __Field::__forward_to_flattened)
			}
			fn pull_value<M: serde::de::MapAccess<'de>>(
				&mut self,
				map_access: &mut M,
				key: Self::Key,
			) -> Result<(), M::Error> {
				match key {
					__Field::__field0 => {
						if serde::__private::Option::is_some(&self.field0) {
							return serde::__private::Err(<M::Error as serde::de::Error>::duplicate_field("a"));
						}
						self.field0 = serde::__private::Some(serde::de::MapAccess::next_value::<i32>(map_access)?);
					}
					__Field::__forward_to_flattened => {
						// necessary if sub-struct doesn't entirely consume the map - is this really possible though?
						serde::de::MapAccess::next_value::<serde::de::IgnoredAny>(map_access)?;
					}
				}
				Ok(())
			}
		}

		let mut flatten_spec = __FlattenSpec {
			field0: serde::__private::None,
		};

		let __field1: B = serde::Deserialize::deserialize(serde_zero_alloc_flatten::FlattenDeserializer::new(
			__deserializer,
			&mut flatten_spec,
		))?;

		let __field0 = match flatten_spec.field0 {
			serde::__private::Some(__field0) => __field0,
			serde::__private::None => serde::__private::de::missing_field("a")?,
		};

		serde::__private::Ok(A {
			a: __field0,
			b: __field1,
		})
	}
}
