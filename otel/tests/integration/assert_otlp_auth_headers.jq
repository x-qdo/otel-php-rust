def attribute_map:
  reduce (. // [])[] as $attribute ({}; .[$attribute.key] = $attribute.value);

[
  .[]
  | .resourceSpans[]
  | . as $resource_spans
  | .scopeSpans[]
  | .spans[]
  | {
      service: ($resource_spans.resource.attributes | attribute_map)["service.name"].stringValue,
      source: (.attributes | attribute_map)["auth.header.source"].stringValue,
      name
    }
] as $spans
| if ($spans | sort_by(.service)) == ([
    {
      service: "auth-grpc-global-header",
      source: "global",
      name: "authenticated-export"
    },
    {
      service: "auth-http-traces-header",
      source: "traces-specific",
      name: "authenticated-export"
    }
  ] | sort_by(.service))
  then $spans
  else error("authenticated OTLP exports did not match expected services and attributes")
  end
