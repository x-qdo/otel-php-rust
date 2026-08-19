def attribute_map:
  reduce (. // [])[] as $attribute ({}; .[$attribute.key] = $attribute.value);

def fail_unless($condition; $message):
  if $condition then . else error($message) end;

def batches:
  [
    .[]
    | .resourceSpans[]
    | . as $resource_spans
    | .scopeSpans[]
    | {
        resource: ($resource_spans.resource.attributes | attribute_map),
        scope: .scope,
        schema_url: .schemaUrl,
        spans: .spans
      }
  ];

def check_service($all_batches; $service):
  [$all_batches[] | select(.resource["service.name"].stringValue == $service)] as $service_batches
  | [$service_batches[].spans[]] as $spans
  | [$spans[] | select(.name == "conformance-root")] as $roots
  | [$spans[] | select(.name | startswith("conformance-child-"))] as $children
  | [$spans[] | .events[]? | select(.name == "exception")] as $exceptions
  | ($service_batches[0].resource // {}) as $resource
  | ($service_batches[0].scope // {}) as $scope
  | ($scope.attributes | attribute_map) as $scope_attributes
  | ($children[0].attributes | attribute_map) as $child_attributes
  | ($children[0].events[] | select(.name == "queue.receive") | .attributes | attribute_map) as $event_attributes
  | ($children[0].links[0].attributes | attribute_map) as $link_attributes
  | ($exceptions[0].attributes | attribute_map) as $exception_attributes
  | fail_unless(($service_batches | length) == 2; $service + ": expected exactly two batch exports")
  | fail_unless(all($service_batches[]; (.spans | length) <= 3); $service + ": export exceeded configured batch size")
  | fail_unless(($spans | length) == 6; $service + ": expected six exported spans")
  | fail_unless(($roots | length) == 1; $service + ": expected one root span")
  | fail_unless(($children | length) == 5; $service + ": expected five child spans")
  | fail_unless(all($children[]; .parentSpanId == $roots[0].spanId); $service + ": child parent span id was not propagated")
  | fail_unless(all($spans[]; .traceId == $roots[0].traceId); $service + ": trace id was not propagated")
  | fail_unless(all($spans[]; (.flags % 2) == 1); $service + ": sampled trace flag was not propagated")
  | fail_unless($resource["service.version"].stringValue == "9.9.9"; $service + ": service.version resource attribute missing")
  | fail_unless($resource["deployment.environment.name"].stringValue == "test"; $service + ": deployment environment resource attribute missing")
  | fail_unless($scope.name == "otel-rust-conformance"; $service + ": instrumentation scope name missing")
  | fail_unless($scope.version == "1.2.3"; $service + ": instrumentation scope version missing")
  | fail_unless($service_batches[0].schema_url == "https://example.test/schema"; $service + ": scope schema URL missing")
  | fail_unless($scope_attributes["scope.attribute"].stringValue == "preserved"; $service + ": scope attribute missing")
  | fail_unless($child_attributes["iteration"].intValue == "0"; $service + ": integer span attribute lost its type")
  | fail_unless($child_attributes["cache.hit"].boolValue == true; $service + ": boolean span attribute lost its type")
  | fail_unless($child_attributes["duration.factor"].doubleValue == 1.5; $service + ": float span attribute lost its type")
  | fail_unless(($child_attributes["labels"].arrayValue.values | map(.stringValue)) == ["alpha", "beta"]; $service + ": string-array span attribute missing")
  | fail_unless(($child_attributes["counts"].arrayValue.values | map(.intValue)) == ["1", "2", "3"]; $service + ": integer-array span attribute missing")
  | fail_unless($event_attributes["attempt"].intValue == "1"; $service + ": event attribute missing")
  | fail_unless($event_attributes["redelivered"].boolValue == false; $service + ": boolean event attribute missing")
  | fail_unless($children[0].links[0].traceId == "2b4ef3412d587ce6e7880fb27a316b8c"; $service + ": span link context missing")
  | fail_unless($link_attributes["link.kind"].stringValue == "retry"; $service + ": string link attribute missing")
  | fail_unless($link_attributes["link.attempt"].intValue == "2"; $service + ": integer link attribute missing")
  | fail_unless($children[0].status.code == 2 and $children[0].status.message == "child failed"; $service + ": error status missing")
  | fail_unless($exception_attributes["exception.message"].stringValue == "export failure example"; $service + ": exception message missing")
  | fail_unless($exception_attributes["exception.type"].stringValue == "RuntimeException"; $service + ": exception type missing")
  | fail_unless(($exception_attributes["exception.stacktrace"].stringValue | length) > 0; $service + ": exception stack trace missing")
  | {
      service: $service,
      batches: ($service_batches | map(.spans | length)),
      spans: ($spans | length),
      trace_id: $roots[0].traceId
    };

batches as $all_batches
| [
    check_service($all_batches; "conformance-grpc"),
    check_service($all_batches; "conformance-http-protobuf")
  ]
