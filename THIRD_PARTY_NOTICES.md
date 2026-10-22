# Dependency and distribution notes

The original LICENSE (MIT) is retained for the authored OpenDAW source. No original
copyright notice has been removed. This does not change third-party terms.

Native libraries are built from this repository and linked to operating-system
facilities. No miniaudio, JUCE, Tracktion, CPAL, external plugin SDK, font or binary
third-party library has been bundled into this archive.

The Rust candidate declares these direct versions (not a resolved dependency tree):

- serde 1.0.228 (with derive)
- serde_json 1.0.145
- slint 1.18.1
- slint-build 1.18.1

Consult each downloaded package's license and notice files. Slint offers multiple
license options; the application includes the AboutSlint widget under
Help → About OpenDAW, accessible from its top-level menu, but that alone is
not a legal/compliance determination. Select and document the appropriate option,
audit transitive dependency notices and produce a resolved dependency inventory
before distribution. No redistribution approval or store eligibility is claimed.

Primary documentation consulted during development:

- https://docs.rs/slint/1.18.1/slint/
- https://docs.rs/slint/1.18.1/slint/docs/cargo_features/
- https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/
- https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/ios/
- https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/spinbox/
- https://docs.slint.dev/latest/docs/slint/reference/global-functions/math/
- https://slint.dev/terms-and-conditions
- https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize
- https://learn.microsoft.com/en-us/windows/win32/coreaudio/audclnt-streamflags-xxx-constants
- https://developer.apple.com/documentation/audiotoolbox/audiounitsetproperty(_:_:_:_:_:_:)

These sources informed interface choices. Reading API documentation is not a
substitute for compiling or testing the platform integrations.
