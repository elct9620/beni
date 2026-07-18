# Changelog

## [0.10.0](https://github.com/elct9620/beni/compare/v0.9.0...v0.10.0) (2026-07-18)


### Features

* **beni:** measure the get_args specifier vocabulary as a coverage lens ([a1ea5df](https://github.com/elct9620/beni/commit/a1ea5df9ae53eafee8e44644c99522b76d115727))
* **beni:** read keyword arguments as a separate bucket ([a583b5e](https://github.com/elct9620/beni/commit/a583b5eb4dcb3b536aa08caf9a51248336067809))

## [0.9.0](https://github.com/elct9620/beni/compare/v0.8.1...v0.9.0) (2026-07-16)


### Features

* **beni:** add Array::entries index walk ([844ab18](https://github.com/elct9620/beni/commit/844ab18cf1ddd945856c693c6e9d3e80f1104ae6))
* **beni:** graduate owned rest-arg format projections ([b3063c2](https://github.com/elct9620/beni/commit/b3063c23bd8d710e4aea256c44996450c00a26ac))


### Reverts

* **beni:** withdraw owned rest-arg format projections ([d04fb52](https://github.com/elct9620/beni/commit/d04fb5241f072a19ef7e7fbd9fdd19a632211c4f))

## [0.8.1](https://github.com/elct9620/beni/compare/v0.8.0...v0.8.1) (2026-07-03)


### Bug Fixes

* **beni-sys:** reject flags.mak layouts the token scan cannot read ([bed80f1](https://github.com/elct9620/beni/commit/bed80f152a3f651a4d1df36813c9521110f9c566))
* **beni:** guard const and class-variable access on non-class receivers ([3e64ae8](https://github.com/elct9620/beni/commit/3e64ae820fd94accc48b35b111a2ab3a69a46c71))
* **beni:** mark set_pending_exc unsafe and pin the exception-slot contract ([d21ff98](https://github.com/elct9620/beni/commit/d21ff98dc1ec5daef6a2152dbda01a5cc4bfa589))
* **beni:** saturate out-of-width argc instead of truncating ([e30bede](https://github.com/elct9620/beni/commit/e30bede2426b45655e35213cb64db2f495d219eb))
* **beni:** walk instance variables against a protected snapshot ([3e38d4f](https://github.com/elct9620/beni/commit/3e38d4fd9065e1619699d1da5bd652c51e7823b9))

## [0.8.0](https://github.com/elct9620/beni/compare/v0.7.0...v0.8.0) (2026-06-21)


### ⚠ BREAKING CHANGES

* **beni:** the six symbol-name reads now return owned values — Mrb::sym_name / Symbol::name and Mrb::sym_dump / Symbol::dump return Option<String>, Mrb::sym_name_len / Symbol::name_bytes return Option<Vec<u8>>, where each previously returned a borrowed &'static str or &'static [u8].
* **beni:** `RClass::data_wrap` returns `Result<Value, Error>` instead of `Value`. Correct use against a marked class yields `Ok`; an unmarked class yields `Err` with the unwrapped value reclaimed.

### Features

* **api:** rank ungraduated C API by mrbgems usage ([81b2125](https://github.com/elct9620/beni/commit/81b21255f7e14d3f25c17a1ec0e79a00c08b85cb))
* **beni:** accept a symbol-or-name key for value dispatch ([e70eacb](https://github.com/elct9620/beni/commit/e70eacba390bd712d89e2f5f271e35114e478529))
* **beni:** graduate anonymous class and module creation ([2b49324](https://github.com/elct9620/beni/commit/2b493240d1954ff37c5deb40b3c7e65c612dc4ad))
* **beni:** graduate building an exception from a string value ([7b0a969](https://github.com/elct9620/beni/commit/7b0a96965e4f7b2a6e777a6f7c56b722bb496225))
* **beni:** graduate interning a runtime byte slice ([2be55ed](https://github.com/elct9620/beni/commit/2be55ed7dc57b3f89d5795b7e2b5fab379520120))
* **beni:** graduate iterating a value's instance variables ([8349ace](https://github.com/elct9620/beni/commit/8349acef3e9610f935c25befd5e07ce886b0ea5f))
* **beni:** graduate method removal ([721ca6d](https://github.com/elct9620/beni/commit/721ca6db03b43c2cdab47ef27bfe8ae7482f3d97))
* **beni:** graduate method undefinition ([ab429a0](https://github.com/elct9620/beni/commit/ab429a0aeb10d048a55e7fee6f82c89caee1b050))
* **beni:** graduate MRB_ARGS_NONE to the typed surface ([d442072](https://github.com/elct9620/beni/commit/d442072855e9fc1da13773f12e23b1fc838c42ce))
* **beni:** graduate no-copy static-buffer string construction ([c377e82](https://github.com/elct9620/beni/commit/c377e826dc7a630fa8d4c3f8bfc5393c3b188a8c))
* **beni:** graduate numeric conversion to the typed surface ([c7de524](https://github.com/elct9620/beni/commit/c7de5244e3375e8b838672687890a6991bd65755))
* **beni:** graduate optional method arity ([e469534](https://github.com/elct9620/beni/commit/e4695343f7796734544f8acec3834da257b1d37d))
* **beni:** graduate prepending a module ([50710ce](https://github.com/elct9620/beni/commit/50710ced88fcdfddd13c3741fcd7d85172360c96))
* **beni:** graduate string capacity construction and string append ([eca35bc](https://github.com/elct9620/beni/commit/eca35bcf93f1db3246d74ef75a196233563c43bd))
* **beni:** graduate string resize, concat, and substr ([3181343](https://github.com/elct9620/beni/commit/318134344cf35c5977f74311b9e3b4378b03859d))
* **beni:** graduate the argument-array read ([d439238](https://github.com/elct9620/beni/commit/d4392387b9d0c03af48dadbab4fd7c00ff2c7356))
* **beni:** graduate the argument-count error constructor ([dbb3711](https://github.com/elct9620/beni/commit/dbb3711e4737e7f1388d738aace593dd8f83e505))
* **beni:** graduate the array join ([2214780](https://github.com/elct9620/beni/commit/22147807d0b297d44e1b3cb9789eff36df41cd84))
* **beni:** graduate the array replace ([1996cc6](https://github.com/elct9620/beni/commit/1996cc6f2e714c1ab252428f38c5b1ba62f7d12a))
* **beni:** graduate the array resize and pair constructor ([a222f94](https://github.com/elct9620/beni/commit/a222f949e2f8cd89751b5e5cacbed5594813c4ac))
* **beni:** graduate the block method argument ([a570af0](https://github.com/elct9620/beni/commit/a570af023663c1a523e17b1ac51fa5494735df3e))
* **beni:** graduate the block-given predicate ([fbf9c5d](https://github.com/elct9620/beni/commit/fbf9c5de483d87de93cb5811e3fb2d5b8a57898f))
* **beni:** graduate the block-passing dispatch ([2c465de](https://github.com/elct9620/beni/commit/2c465de1764cc0b817fd3a68a7e840e65b803f03))
* **beni:** graduate the built-in exception-class lookup ([5181b46](https://github.com/elct9620/beni/commit/5181b4649245c2dbf8ecce3be5f5d75bf524e33a))
* **beni:** graduate the byte-equality test ([3dd0f72](https://github.com/elct9620/beni/commit/3dd0f726dc0f696a034c03a57ddfcff8805c9467))
* **beni:** graduate the C-string append ([85e517b](https://github.com/elct9620/beni/commit/85e517bd815acb615590fc739ec7f96601b011dd))
* **beni:** graduate the call-frame argument reads ([e3e5c1f](https://github.com/elct9620/beni/commit/e3e5c1faa43d4a392ea2992a9b0c3c262f4392e8))
* **beni:** graduate the class-handle real-class resolution ([2e00c8c](https://github.com/elct9620/beni/commit/2e00c8c08ed1634631e73b1ef0dcb252ea12449e))
* **beni:** graduate the class-variable presence test ([f47edbd](https://github.com/elct9620/beni/commit/f47edbd20f196ec72ea5e084b442e356f4ca2b2b))
* **beni:** graduate the class-variable read and instance-variable presence ([3b6997d](https://github.com/elct9620/beni/commit/3b6997d9f5273d2c3fc7ae300023b430f696fcb2))
* **beni:** graduate the class/module defined predicate ([17e71fd](https://github.com/elct9620/beni/commit/17e71fd16b883f4bc3327cefa07b49ef7d1f34c6))
* **beni:** graduate the constant removal ([41cf65a](https://github.com/elct9620/beni/commit/41cf65a8c77d877bba06c6eaec65aec0dea919c2))
* **beni:** graduate the context-free Ruby-source eval ([62c0d25](https://github.com/elct9620/beni/commit/62c0d25b32999fd8f1fdd78f9ae10b22b65ba6ca))
* **beni:** graduate the default to_s render ([61e7d72](https://github.com/elct9620/beni/commit/61e7d72d93e5625a0e0e081df5a479a52afd0a57))
* **beni:** graduate the direct constant presence test ([617c48b](https://github.com/elct9620/beni/commit/617c48b21904a01fd27de379c03fac3d806f9dfa))
* **beni:** graduate the float-to-integer conversion ([7880249](https://github.com/elct9620/beni/commit/7880249ea89393daeb0ee96a7b48cbc13a041427))
* **beni:** graduate the frozen-state precondition guard ([4cf74c4](https://github.com/elct9620/beni/commit/4cf74c4d24e868d521a016e1b9cc7b3d69e80ae9))
* **beni:** graduate the hash key-value iteration ([85d7dc9](https://github.com/elct9620/beni/commit/85d7dc908359b2e9e5a3e04daf834f9f470e08da))
* **beni:** graduate the in-place subsequence splice ([fe281cc](https://github.com/elct9620/beni/commit/fe281ccf666c434949aa0c7d39cc68ecbbe7d835))
* **beni:** graduate the instance-variable removal ([e9db9b8](https://github.com/elct9620/beni/commit/e9db9b8fe2a4c9a07761fb7feb568e6cdcd13b5e))
* **beni:** graduate the integer-to-string render ([153acfb](https://github.com/elct9620/beni/commit/153acfb614c3e5de5f8f9b2a074ae327b4879d7d))
* **beni:** graduate the intern presence test ([a6b76f9](https://github.com/elct9620/beni/commit/a6b76f962cd53d33fd69eb0e3174d7725198cfcb))
* **beni:** graduate the lenient integer parse ([92d58a3](https://github.com/elct9620/beni/commit/92d58a36d55ff94c8fa34b42f18358e50e63f91f))
* **beni:** graduate the module, range, and exception tag predicates ([9d8a46f](https://github.com/elct9620/beni/commit/9d8a46fa26ce6421c6ca39a64efe991c9d0bbbd7))
* **beni:** graduate the Mrb::rescue combinator ([2b2daab](https://github.com/elct9620/beni/commit/2b2daab9dcd893d55a83dd3374ba31d4ae48dd32))
* **beni:** graduate the namespaced module lookup ([aaade85](https://github.com/elct9620/beni/commit/aaade857940bb372c12d6e2084871ffdb03c6400))
* **beni:** graduate the no-arg GC triggers ([1b5e4c9](https://github.com/elct9620/beni/commit/1b5e4c99b5b7c7c97895c519977f959a4c128d05))
* **beni:** graduate the no-copy symbol intern ([80fcc90](https://github.com/elct9620/beni/commit/80fcc900f4b470f2eb285f047e792872c9edd526))
* **beni:** graduate the non-mutating string concatenation ([08b16f0](https://github.com/elct9620/beni/commit/08b16f0bb71fc6f16f24a197abbcb7f0d2e308dc))
* **beni:** graduate the numeric arithmetic family ([e55f378](https://github.com/elct9620/beni/commit/e55f3781bde8982ba0ac4264883d7a92411c6c7d))
* **beni:** graduate the numeric ensure-type coercions ([0b8303f](https://github.com/elct9620/beni/commit/0b8303fa126b0b5963270aded5428df72c4395a9))
* **beni:** graduate the preallocated-capacity hash construction ([7ef0852](https://github.com/elct9620/beni/commit/7ef08525d5922e7d988da4db7ff56942a166d6c6))
* **beni:** graduate the qualified class path read ([d3d0fd0](https://github.com/elct9620/beni/commit/d3d0fd0e4bd811b28513ddd412760fb8325a571d))
* **beni:** graduate the raising string-type coercion ([bd05c99](https://github.com/elct9620/beni/commit/bd05c9942d8fbcfb91a079a8ca13add89367140d))
* **beni:** graduate the Range slice computation ([e9be245](https://github.com/elct9620/beni/commit/e9be245b07b50a2584900ba10be555f068ac5b78))
* **beni:** graduate the singleton class read ([437304a](https://github.com/elct9620/beni/commit/437304a259901c2ca32cf8850cbce0900f997228))
* **beni:** graduate the splat coercion ([7bb6ec6](https://github.com/elct9620/beni/commit/7bb6ec6bc1ff2e24facd52479c7b151c83e9e1e4))
* **beni:** graduate the string C-string read ([bc8ba88](https://github.com/elct9620/beni/commit/bc8ba8815fccff7f17cb5eae3bdcda6258c903b6))
* **beni:** graduate the string float parse ([0603cc6](https://github.com/elct9620/beni/commit/0603cc68f03ce820ae7fd8ee1e940716b4c92bab))
* **beni:** graduate the string-intern read ([c03dead](https://github.com/elct9620/beni/commit/c03dead49b20bf5ceaf840b040313bedea3f93e8))
* **beni:** graduate the string-to-integer parse ([8770cb2](https://github.com/elct9620/beni/commit/8770cb26c42938d8c964cffa8dfe1cfdf4bf68ad))
* **beni:** graduate the substring search read ([3d4e6f4](https://github.com/elct9620/beni/commit/3d4e6f42a472037a1cdd9dd62feb379e118ce6d4))
* **beni:** graduate the symbol name and dump reads ([74e839a](https://github.com/elct9620/beni/commit/74e839a9866fd148e8938a9c156e346bd6ce8731))
* **beni:** graduate the symbol-keyed method alias ([5c535e4](https://github.com/elct9620/beni/commit/5c535e4c03de1bf7c8f0d84d6ff267ad8a5a76a4))
* **beni:** graduate the symbol-name string read ([c6f8e69](https://github.com/elct9620/beni/commit/c6f8e69b9e899aef8114f9188413e67fee4b40e8))
* **beni:** graduate the symbol-or-name define/get key ([95e9c56](https://github.com/elct9620/beni/commit/95e9c568d7381889b7a3284956d18e4511092a57))
* **beni:** graduate the top-level module lookup ([e05b4de](https://github.com/elct9620/beni/commit/e05b4de0af19c6d8c22f874a98a8827eab638d2a))
* **beni:** graduate the typed array and hash coercion ([bf218e0](https://github.com/elct9620/beni/commit/bf218e0fa7161158d755753dce8e80d6a31df51f))
* **beni:** graduate the typed comparison ([88f08d5](https://github.com/elct9620/beni/commit/88f08d587129714b81a293745b8ae2ac7ca6fd60))
* **beni:** graduate the typed range ([3000c85](https://github.com/elct9620/beni/commit/3000c854440477e65407f0466ca306330f97a319))
* **beni:** graduate the value object identity id ([3dcc1bc](https://github.com/elct9620/beni/commit/3dcc1bc4ab8c005be3f1db5dca960a8ce726c5f4))
* **beni:** graduate the value-to-symbol coercion ([4339d23](https://github.com/elct9620/beni/commit/4339d23487f92d18a188024b87d05d852516dae2))
* **beni:** graduate the variable write companions ([b5898a9](https://github.com/elct9620/beni/commit/b5898a9d013495a0329479a967bff95ba506376a))
* **beni:** record mrb_str_cat_lit on the typed surface ([fe602a0](https://github.com/elct9620/beni/commit/fe602a0e7132e39e7464980426a7cbd62776d932))


### Bug Fixes

* **beni:** contain a panicking payload drop in the GC release hook ([f9769dc](https://github.com/elct9620/beni/commit/f9769dccb9cdf42b0106ae889b3c3f6baf6ed21d))
* **beni:** copy class names out of mruby's GC-managed temporary ([0660194](https://github.com/elct9620/beni/commit/06601948059d9ae7774874b6ba459b254a1df03c))
* **beni:** copy symbol names out instead of borrowing the shared buffer ([91e5df9](https://github.com/elct9620/beni/commit/91e5df990ca64050168a67d81c38c837a2b344e1))
* **beni:** make data carrier wrapping fallible ([0b3ac32](https://github.com/elct9620/beni/commit/0b3ac32f8f0fcc2d3d4459d681d17f253afd2a1b))
* **beni:** protect Hash::each against the in-walk modify raise ([7b66c82](https://github.com/elct9620/beni/commit/7b66c82f791e6152d0e8338830e0f16b632ead90))
* **beni:** rank Value::cmp by the sign of &lt;=&gt;, not its magnitude ([a0deab4](https://github.com/elct9620/beni/commit/a0deab4936072b2f7bfe92535905e96f28ae9b04))
* **beni:** saturate out-of-width range length instead of wrapping ([4b58218](https://github.com/elct9620/beni/commit/4b582185eafdf0d09e6886d536231da4f8efe10b))
* **beni:** saturate out-of-width substr/index offsets instead of wrapping ([bdb1461](https://github.com/elct9620/beni/commit/bdb1461f40743e7b34420fa701a026a1393fbd61))

## [0.7.0](https://github.com/elct9620/beni/compare/v0.6.1...v0.7.0) (2026-06-14)


### Features

* **beni:** add Error::new for handler-authored exceptions ([4842496](https://github.com/elct9620/beni/commit/48424965e4c771c2b9097e63ce973c42799a79a7))
* **beni:** convert an mruby string to owned bytes ([ec5e175](https://github.com/elct9620/beni/commit/ec5e1756d1c6bb7620403c0cf38fe61ac13ad5e4))
* **beni:** graduate array index-write and capacity/slice construction ([68af55d](https://github.com/elct9620/beni/commit/68af55d3330020356aaaf3c202f7e85c251c1aa8))
* **beni:** graduate module functions; record class-method alignment ([c7c6c47](https://github.com/elct9620/beni/commit/c7c6c472c05a09830e7d022b4a1bc27c267e773b))
* **beni:** graduate module inclusion onto the Module trait ([c751c3f](https://github.com/elct9620/beni/commit/c751c3f152a9f8384788e6fb4467cb2bf44effab))
* **beni:** graduate string append and owned-String conversion ([6035360](https://github.com/elct9620/beni/commit/60353601fd04f5d94524b5bb73ce80a304107ad8))
* **beni:** graduate the typed array's remove/extend/clear/dup surface ([842d4f2](https://github.com/elct9620/beni/commit/842d4f227be6ea0f71c09691d5ef1b3e0e439165))
* **beni:** graduate the typed hash's clear onto the Hash surface ([a1ef297](https://github.com/elct9620/beni/commit/a1ef297dab8e0463462f78849b099a8dde78ba8c))
* **beni:** graduate the typed hash's read, fetch, delete, and merge surface ([4af31f1](https://github.com/elct9620/beni/commit/4af31f1d2589c334c3c7204f74907c44c86d0b85))
* **beni:** graduate the typed string's dup and byte compare ([05ea9ef](https://github.com/elct9620/beni/commit/05ea9ef4793411c4f7ace2352b3a17e38075bae1))
* **beni:** graduate the typed value's inspect render ([e6f7bb0](https://github.com/elct9620/beni/commit/e6f7bb042aee9869b94aa338ab7346bd61f13822))
* **beni:** graduate the value reflection surface (class, is_a?, freeze) ([66f4dfd](https://github.com/elct9620/beni/commit/66f4dfd24daeb4d060ddb492da6533f640ac1a6f))
* **beni:** graduate value comparison (==, eql?, identity) ([f12c645](https://github.com/elct9620/beni/commit/f12c6459929c053e80efc9333139f3688bed42ec))
* **beni:** introduce RString and graduate string ops onto it ([d71c093](https://github.com/elct9620/beni/commit/d71c0939e9e3d6bd9a7f7577765bc03095eef489))
* **beni:** report an RString's byte length on the typed surface ([47c4b48](https://github.com/elct9620/beni/commit/47c4b4830d2a6419cc9fa59fcab35606c9b7b2f8))
* **coverage:** drop non-embedder macros, credit tag predicates ([5c6b656](https://github.com/elct9620/beni/commit/5c6b65670412eb4d7bb3c8e67d9c46c4d39e604a))


### Bug Fixes

* **beni:** make data_reinit a safe no-op on a non-carrier value ([e5339f2](https://github.com/elct9620/beni/commit/e5339f22a0b8d253ff361a5764a51375f9edae2f))
* **beni:** normalize a symbol toolchain name like a target ([ff1beb7](https://github.com/elct9620/beni/commit/ff1beb746c44cf298a6c88e22b600b6a7a5774d9))
* **beni:** protect str_cat against a frozen receiver ([9b78cc3](https://github.com/elct9620/beni/commit/9b78cc3f51ae0f78115fd6b9285931d6e083c044))
* **beni:** protect the dispatching and mutating typed ops against raises ([8b83ab9](https://github.com/elct9620/beni/commit/8b83ab936d5f696e0fb58b343af7dedffd3776e0))
* **beni:** read a String subclass to_s result by tag ([66589b9](https://github.com/elct9620/beni/commit/66589b9ff21f57982250f9de5ec18ce3b13b534a))
* **beni:** route class obj_new through protect ([124cd29](https://github.com/elct9620/beni/commit/124cd29288ed4f5c42a151888350e248df48e58e))
* **beni:** route Hash::get through protect ([e0a9c92](https://github.com/elct9620/beni/commit/e0a9c92e42f06e9d09b977de4975c4c81dc4327e))
* **beni:** route value dup, clone, and string coercion through protect ([3ac8cc2](https://github.com/elct9620/beni/commit/3ac8cc2cb3bf8a4bc6a44d7ecad8b4f7cdcf9385))
* **beni:** route value iv_set and const_get through protect ([d46f18c](https://github.com/elct9620/beni/commit/d46f18cdcab51e8907123a405cf7225ee21f505c))
* **beni:** saturate the exception message length like the string factory ([11eb7b9](https://github.com/elct9620/beni/commit/11eb7b91bb7f3b935fbdd1e4ca5863b0d4ba8a8c))
* **coverage:** credit is_integer to integer_p, not fixnum_p ([32f064f](https://github.com/elct9620/beni/commit/32f064fe810b2181b826251b447dbfc16208cf31))
* **coverage:** label trait graduations by their trait ([be598c6](https://github.com/elct9620/beni/commit/be598c6b4e59a885a6eaae08ee64c3900c9cb198))

## [0.6.1](https://github.com/elct9620/beni/compare/v0.6.0...v0.6.1) (2026-06-11)


### Bug Fixes

* **wasi:** target wasm32-wasip1 instead of the deprecated wasi triple ([9603c38](https://github.com/elct9620/beni/commit/9603c385b77e2349400c459c5073cac73922a4af))

## [0.6.0](https://github.com/elct9620/beni/compare/v0.5.0...v0.6.0) (2026-06-09)


### Features

* **beni:** graduate object dup and clone onto Value ([fd5c9c7](https://github.com/elct9620/beni/commit/fd5c9c704af3d63bbdf6c583078f779ba1985a8d))
* **beni:** install a Rust payload into a bare data carrier ([d4c059d](https://github.com/elct9620/beni/commit/d4c059dd7d5c3190e93fef7e332ea7fc258f1e27))
* **beni:** read mruby booleans through the typed surface ([6b847fc](https://github.com/elct9620/beni/commit/6b847fce127bf244bb14867fa5ad95c4b47a21fb))
* **coverage:** track mruby C API binding coverage ([c71f832](https://github.com/elct9620/beni/commit/c71f832ed05616d8545076d143d7a27ac14bad08))

## [0.5.0](https://github.com/elct9620/beni/compare/v0.4.0...v0.5.0) (2026-06-08)


### Features

* **beni:** add a typed Symbol newtype ([d38d44c](https://github.com/elct9620/beni/commit/d38d44cc70e39046be87eec8ca3ced8b09da81af))
* **beni:** add Array::len and is_empty ([ad43e0b](https://github.com/elct9620/beni/commit/ad43e0bbaf346fbcad1af0ee6d7a7338ee2cf3a4))
* **beni:** add Module::alias_method ([8f0e176](https://github.com/elct9620/beni/commit/8f0e17693b7cb87f93dd21d5274dbcc360e3763a))
* **beni:** add String and block-rest get_args formats ([ac3272d](https://github.com/elct9620/beni/commit/ac3272da4ba76b12681b1de42648bc96349a881c))
* **beni:** expose string, constant, and exception seams ([a8b702f](https://github.com/elct9620/beni/commit/a8b702ff92549308c2f22d698d7976d85976cd85))
* **beni:** wrap Rust state behind a Ruby class via CDATA ([964fedf](https://github.com/elct9620/beni/commit/964fedf92880cef96a8f61b651b9a189fdccf69f))

## [0.4.0](https://github.com/elct9620/beni/compare/v0.3.0...v0.4.0) (2026-06-08)


### Features

* **beni:** add a typed Proc handle with a protected yield ([e69aee7](https://github.com/elct9620/beni/commit/e69aee7ca572c2aac1e8ec9c24912f854934f5ee))
* **beni:** expose the break view and current call-info index ([30161c6](https://github.com/elct9620/beni/commit/30161c6bf58d6bc277546ae8f39aa3dc299d8425))

## [0.3.0](https://github.com/elct9620/beni/compare/v0.2.0...v0.3.0) (2026-06-07)


### ⚠ BREAKING CHANGES

* **beni:** take Array::entry's index as isize

### Features

* **beni:** add checked FromValue downcast for RClass ([b21db5b](https://github.com/elct9620/beni/commit/b21db5b553f584ddd460733a2db37bb7e2fec8e2))
* **beni:** add checked FromValue downcasts for Array and Hash ([eb30b0a](https://github.com/elct9620/beni/commit/eb30b0ace3e821d0cf1a6071360958c76dbf616e))
* **beni:** take Array::entry's index as isize ([0c43f9c](https://github.com/elct9620/beni/commit/0c43f9c0282ec22b4646e20485a6f7f3fd7c76b1))

## [0.2.0](https://github.com/elct9620/beni/compare/v0.1.0...v0.2.0) (2026-06-07)


### Features

* **beni:** add ArenaScope for GC arena bracketing ([ec8e1e4](https://github.com/elct9620/beni/commit/ec8e1e416e3a4dc2433281dfe9945916fcf98081))
* **beni:** add Module::define_private_method ([86c5d24](https://github.com/elct9620/beni/commit/86c5d241aea6ecd27d73bcdcf37678d07fa054fc))
* **beni:** add Mrb::gv_get as the read counterpart of gv_set ([55f1bf0](https://github.com/elct9620/beni/commit/55f1bf024f1ab761450f602f9aa2f2f29932d25d))

## 0.1.0 (2026-06-06)


### ⚠ BREAKING CHANGES

* require Ruby >= 3.3
* **beni-sys:** align bindgen with the archive via libmruby.flags.mak
* **beni:** default the build to mruby's upstream config, not the repo's

### Features

* add Beni::Builder driving mruby's own rake ([2be29d3](https://github.com/elct9620/beni/commit/2be29d3d1fa353804413b92203758232796fad3b))
* add Beni::Tasks exposing the beni:* rake namespace ([2833c23](https://github.com/elct9620/beni/commit/2833c23a7a01efea185cfcd165f4499e5ffc0cf7))
* add RBS type checking and Claude Code quality hooks ([707e692](https://github.com/elct9620/beni/commit/707e6920f26b21ff4a67f8a31691fc21e1d5dd81))
* **beni-sys:** align bindgen with the archive via libmruby.flags.mak ([8d019dc](https://github.com/elct9620/beni/commit/8d019dc40fb0c6f7cf5b4139ac27b6a3b7d1459a))
* **beni:** compile the full wrapper API surface in placeholder mode ([00a429d](https://github.com/elct9620/beni/commit/00a429dfe524b7daaeb72765841136e2e92ed83a))
* **beni:** generate self-contained build configs via rake beni:config ([51c04c2](https://github.com/elct9620/beni/commit/51c04c214a1cc5b2766b68a1e54937cf00a5a69d))
* **beni:** register typed Rust functions with method! and seal the panic boundary ([b1706a2](https://github.com/elct9620/beni/commit/b1706a2aa2aa9c8550846ea2e4d139720ec1aa34))
* **beni:** ship Ruby surfaces as Gem implementations ([37b6260](https://github.com/elct9620/beni/commit/37b6260b235b4510d145ce8dd933e2f62bf265d8))
* **beni:** surface mruby rejections as Err through magnus-shaped handles ([ccd2809](https://github.com/elct9620/beni/commit/ccd280974700e3c3943e541c4bf72260eaad74be))
* **config:** generate the build config from the staged upstream default ([5429c54](https://github.com/elct9620/beni/commit/5429c542ceb58456be8b529c1422d4efe18ded07))
* **dsl:** add the declarative configuration vocabulary and resolution ([a8cbe48](https://github.com/elct9620/beni/commit/a8cbe483e7ea877ae59277056ffae6f282c5fe54))
* port kobako's rake chain as the compile-verification harness ([a0e69f2](https://github.com/elct9620/beni/commit/a0e69f20609fb293b317f4ee206169dd3f543f37))
* port the vendor pipeline into Beni::Vendor ([4387e6b](https://github.com/elct9620/beni/commit/4387e6be8432975ebf69cdcd2c35bd02d38060d4))
* ship the mruby build configs with the gem ([88d79d4](https://github.com/elct9620/beni/commit/88d79d4bbc4f28c12a767ae078d8ea6775e0fdc3))
* **sys:** make archive discovery env-driven with no vendor fallback ([389d608](https://github.com/elct9620/beni/commit/389d6088644559ac3f53505b4fe9bfcc5f25b298))
* **tasks:** switch Beni::Tasks to the declarative DSL ([7eaf07f](https://github.com/elct9620/beni/commit/7eaf07f8f2b6b57f6cb93aa37edebe680ee0b7b9))
* **test:** add default_host consumer-scenario harness ([9d7d6d3](https://github.com/elct9620/beni/commit/9d7d6d3b339bcda266643ce3535efeb9b720cf0d))
* **test:** exercise the generate-config chain as a consumer scenario ([2df1490](https://github.com/elct9620/beni/commit/2df1490ee19b161d9d8fb171d69669c7016c8fdd))
* **vendor:** stage the wasi toolchain file into the mruby tree ([e081b69](https://github.com/elct9620/beni/commit/e081b694dfbe45b35ac58e45eb22fd5fe2997447))
* **vendor:** vendor built-in version-checksum pairs and inject selection into toolchains ([b3edf10](https://github.com/elct9620/beni/commit/b3edf103351814f8e67d7d2a3f4fbf5fff6ec223))


### Bug Fixes

* **beni:** gate Mrb's repr(transparent) on mruby_linked, not wasm32 ([7d8c949](https://github.com/elct9620/beni/commit/7d8c9499e1365e6bb2c6551d1e918738163db1cb))
* **beni:** prefix the remaining runtime errors for CI log grepping ([ffd799a](https://github.com/elct9620/beni/commit/ffd799a67bcdc1ca9dbb92d106c14f333afe14b8))
* **beni:** treat init-failed mruby states and missing configs as errors ([e4c0bf5](https://github.com/elct9620/beni/commit/e4c0bf5c406717c7129901a11d5fb2791b33bf3e))
* **beni:** type mrb_get_args count out-params as mrb_int, not c_int ([b21ee22](https://github.com/elct9620/beni/commit/b21ee225577d6b2bba8176836e426e7e25611b93))
* **gemspec:** give source_code_uri its own URL ([3d3da60](https://github.com/elct9620/beni/commit/3d3da6092d0c7f222558a822f3b7c5c1660d0ff0))
* **gemspec:** ship only the consumer-facing files and link the changelog ([176f18a](https://github.com/elct9620/beni/commit/176f18a77fb01f18502a0e0b6c1b237470e0d0fc))
* **release:** pin the first release to 0.1.0 and drop the dead lock updaters ([5bd6106](https://github.com/elct9620/beni/commit/5bd61065d61f3e00cfbd6918d7dd803f2e9e11b4))


### Miscellaneous Chores

* require Ruby &gt;= 3.3 ([de636e8](https://github.com/elct9620/beni/commit/de636e87163474f84f19e7163f22c51a1ec9dc22))


### Code Refactoring

* **beni:** default the build to mruby's upstream config, not the repo's ([0cafae1](https://github.com/elct9620/beni/commit/0cafae18e17350c0cbbd5d0048c1131e450cd573))
