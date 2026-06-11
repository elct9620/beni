# Changelog

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
