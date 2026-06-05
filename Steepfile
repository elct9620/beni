# frozen_string_literal: true

target :lib do
  signature "sig"

  check "lib"

  # The build-config template is a shipped asset the consumer copies
  # and mruby's build DSL evaluates — never loaded by the gem, so the
  # MRuby::* constants it references live outside the typed surface.
  ignore "lib/beni/templates"

  library "digest"
  library "open-uri"
  library "net-http"
  library "uri"
  library "socket"
end
