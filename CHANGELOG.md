# Changelog

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
