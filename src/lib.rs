use std::marker::PhantomData;

use serde::de::*;

pub trait FlattenSpec<'de> {
	type Key: Deserialize<'de>;
	fn should_forward_to_flatten_field(key: &Self::Key) -> bool;
	/// Pull value for that given key and store it where relevant
	///
	/// This should *not* error if variant is such that `is_for_flatten` is true, it should ignore in that case.
	fn pull_value<M: MapAccess<'de>>(&mut self, map_access: &mut M, key: Self::Key) -> Result<(), M::Error>;
}

pub struct FlattenDeserializer<'f, D, F> {
	inner: D,
	flatten_spec: &'f mut F,
}

impl<'f, D, F> FlattenDeserializer<'f, D, F> {
	pub fn new(from_deserializer: D, flatten_spec: &'f mut F) -> Self {
		FlattenDeserializer {
			inner: from_deserializer,
			flatten_spec,
		}
	}
}

impl<'f, 'de, D: Deserializer<'de>, F: FlattenSpec<'de>> Deserializer<'de> for FlattenDeserializer<'f, D, F> {
	type Error = D::Error;

	fn deserialize_any<V>(self, _: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		Err(Error::custom("can only flatten structs and maps"))
	}

	fn deserialize_struct<V>(
		self,
		_: &'static str,
		_: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		self.inner.deserialize_map(DeserializeMapOrStructVisitor {
			inner_struct_or_map_visitor: visitor,
			flatten_spec: self.flatten_spec,
		})
	}

	fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		self.inner.deserialize_map(DeserializeMapOrStructVisitor {
			inner_struct_or_map_visitor: visitor,
			flatten_spec: self.flatten_spec,
		})
	}

	// TODO support enums
	// TODO if necessary make it work if no deserialize_xxx is called (re-drive inner)
	//      or does serde require that this is never the case?

	serde::forward_to_deserialize_any! {
		bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
		bytes byte_buf option unit unit_struct newtype_struct seq tuple
		tuple_struct enum identifier ignored_any
	}
}

struct DeserializeMapOrStructVisitor<'f, V, F> {
	inner_struct_or_map_visitor: V,
	flatten_spec: &'f mut F,
}

impl<'f, 'de, V: Visitor<'de>, F: FlattenSpec<'de>> Visitor<'de> for DeserializeMapOrStructVisitor<'f, V, F> {
	type Value = V::Value;

	fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(formatter, "a map")
	}

	fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
	where
		A: MapAccess<'de>,
	{
		let mut flatten_map_access = FlattenMapAccess {
			inner: map,
			finished: false,
			flatten_spec: self.flatten_spec,
		};
		let res = self.inner_struct_or_map_visitor.visit_map(&mut flatten_map_access)?;
		if !flatten_map_access.finished {
			while let Some(key) = flatten_map_access.inner.next_key::<F::Key>()? {
				flatten_map_access
					.flatten_spec
					.pull_value(&mut flatten_map_access.inner, key)?;
			}
		}
		Ok(res)
	}
}

struct FlattenMapAccess<'f, MA, F> {
	inner: MA,
	finished: bool,
	flatten_spec: &'f mut F,
}
impl<'de, MA: MapAccess<'de>, F: FlattenSpec<'de>> MapAccess<'de> for &'_ mut FlattenMapAccess<'_, MA, F> {
	type Error = MA::Error;

	fn next_key_seed<K>(&mut self, mut seed: K) -> Result<Option<K::Value>, Self::Error>
	where
		K: DeserializeSeed<'de>,
	{
		Ok(loop {
			// Go through all the values that are used by the parent, and only return when finding one that the
			// parent does not capture
			match self.inner.next_key_seed(DeserializeKeySeed::<_, F> {
				key_seed: seed,
				flatten_spec: PhantomData,
			})? {
				None => {
					self.finished = true;
					break None;
				}
				Some(KeyOwner::NotMatchedByFlattenSpec(val)) => break Some(val),
				Some(KeyOwner::MatchedByFlattenSpec { key, unused_seed }) => {
					seed = unused_seed;
					self.flatten_spec.pull_value(&mut self.inner, key)?
				}
			}
		})
	}

	fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
	where
		V: DeserializeSeed<'de>,
	{
		self.inner.next_value_seed::<V>(seed)
	}
}

struct DeserializeKeySeed<K, F> {
	key_seed: K,
	flatten_spec: PhantomData<F>,
}

enum KeyOwner<S, K, FK> {
	MatchedByFlattenSpec { key: FK, unused_seed: S },
	NotMatchedByFlattenSpec(K),
}
impl<'de, K: DeserializeSeed<'de>, F: FlattenSpec<'de>> DeserializeSeed<'de> for DeserializeKeySeed<K, F> {
	type Value = KeyOwner<K, K::Value, F::Key>;

	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: Deserializer<'de>,
	{
		// The received deserializer here is the source deserializer able to provide us with the key
		// for the next element in the map (its deserialize_identifier will give a str)
		deserializer.deserialize_identifier(KeyVisitor::<_, F> {
			key_seed: self.key_seed,
			flatten_spec: PhantomData,
		})
	}
}

struct KeyVisitor<K, F> {
	key_seed: K,
	flatten_spec: PhantomData<F>,
}

impl<'de, K: DeserializeSeed<'de>, F: FlattenSpec<'de>> KeyVisitor<K, F> {
	fn try_outer_fallback_inner_visit<E, V>(self, v: V) -> Result<<Self as Visitor<'de>>::Value, E>
	where
		E: Error,
		V: IntoDeserializer<'de, E>,
		<V as IntoDeserializer<'de, E>>::Deserializer: Copy,
	{
		self.try_outer_fallback_inner_visit_deserializer(v.into_deserializer())
	}

	fn try_outer_fallback_inner_visit_deserializer<D>(
		self,
		deserializer: D,
	) -> Result<<Self as Visitor<'de>>::Value, D::Error>
	where
		D: Deserializer<'de> + Copy,
	{
		let outer_struct_key = <F::Key as Deserialize>::deserialize(deserializer)?;
		Ok(if F::should_forward_to_flatten_field(&outer_struct_key) {
			KeyOwner::NotMatchedByFlattenSpec(self.key_seed.deserialize(deserializer)?)
		} else {
			KeyOwner::MatchedByFlattenSpec {
				key: outer_struct_key,
				unused_seed: self.key_seed,
			}
		})
	}

	fn inner_visit<E, V>(self, v: V) -> Result<<Self as Visitor<'de>>::Value, E>
	where
		E: Error,
		V: IntoDeserializer<'de, E>,
	{
		self.inner_visit_deserializer(v.into_deserializer())
	}

	fn inner_visit_deserializer<D>(self, deserializer: D) -> Result<<Self as Visitor<'de>>::Value, D::Error>
	where
		D: Deserializer<'de>,
	{
		self.key_seed
			.deserialize(deserializer)
			.map(KeyOwner::NotMatchedByFlattenSpec)
	}
}

impl<'de, K: DeserializeSeed<'de>, F: FlattenSpec<'de>> Visitor<'de> for KeyVisitor<K, F> {
	type Value = KeyOwner<K, K::Value, F::Key>;

	fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(
			formatter,
			"key of the struct that contains the flatten attribute or key for the flattened type"
		)
	}

	fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.try_outer_fallback_inner_visit(v)
	}

	fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.try_outer_fallback_inner_visit_deserializer(value::BorrowedStrDeserializer::new(v))
	}

	fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
	where
		E: Error,
	{
		let outer_struct_key = <F::Key as Deserialize>::deserialize(value::StrDeserializer::new(v.as_str()))?;
		Ok(if F::should_forward_to_flatten_field(&outer_struct_key) {
			KeyOwner::NotMatchedByFlattenSpec(self.key_seed.deserialize(value::StringDeserializer::new(v))?)
		} else {
			KeyOwner::MatchedByFlattenSpec {
				key: outer_struct_key,
				unused_seed: self.key_seed,
			}
		})
	}

	fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.try_outer_fallback_inner_visit(v)
	}

	fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.try_outer_fallback_inner_visit_deserializer(value::BorrowedBytesDeserializer::new(v))
	}

	fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
	where
		E: Error,
	{
		let outer_struct_key = <F::Key as Deserialize>::deserialize(value::BytesDeserializer::new(v.as_slice()))?;
		Ok(if F::should_forward_to_flatten_field(&outer_struct_key) {
			#[allow(unreachable_code)]
			KeyOwner::NotMatchedByFlattenSpec(todo!("BytesBufDeserializer does not exist"))
		} else {
			KeyOwner::MatchedByFlattenSpec {
				key: outer_struct_key,
				unused_seed: self.key_seed,
			}
		})
	}

	fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	serde::serde_if_integer128! {
		fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
		where
			E: Error,
		{
			self.inner_visit(v)
		}
	}

	fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	serde::serde_if_integer128! {
		fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
		where
			E: Error,
		{
			self.inner_visit(v)
		}
	}

	fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit(v)
	}

	fn visit_none<E>(self) -> Result<Self::Value, E>
	where
		E: Error,
	{
		todo!()
	}

	fn visit_some<D>(self, _deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: Deserializer<'de>,
	{
		todo!()
	}

	fn visit_unit<E>(self) -> Result<Self::Value, E>
	where
		E: Error,
	{
		self.inner_visit_deserializer(value::UnitDeserializer::new())
	}

	fn visit_newtype_struct<D>(self, _deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: Deserializer<'de>,
	{
		todo!()
	}

	fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
	where
		A: SeqAccess<'de>,
	{
		self.inner_visit_deserializer(value::SeqAccessDeserializer::new(seq))
	}

	fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
	where
		A: MapAccess<'de>,
	{
		self.inner_visit_deserializer(value::MapAccessDeserializer::new(map))
	}

	fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
	where
		A: EnumAccess<'de>,
	{
		self.inner_visit_deserializer(value::EnumAccessDeserializer::new(data))
	}

	fn __private_visit_untagged_option<D>(self, _: D) -> Result<Self::Value, ()>
	where
		D: Deserializer<'de>,
	{
		todo!()
	}
}
