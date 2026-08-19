def attribute_map:
  reduce (. // [])[] as $attribute ({}; .[$attribute.key] = $attribute.value);

[
  .[]
  | .resourceSpans[]
  | . as $resource_spans
  | .scopeSpans[].spans[]
  | {
      name,
      service: ($resource_spans.resource.attributes | attribute_map)["service.name"].stringValue,
      pid: ($resource_spans.resource.attributes | attribute_map)["process.pid"].stringValue
    }
] as $spans
| if ($spans | length) != 2 then
    error("expected exactly two fork test spans")
  elif ([$spans[].pid] | unique | length) != 2 then
    error("parent and child spans did not carry distinct process.pid resources")
  elif ([$spans[].name] | sort) != ["child-process-span", "parent-process-span"] then
    error("fork test span names were not both exported")
  elif ([$spans[].service] | sort) != ["fork-runtime-child", "fork-runtime-parent"] then
    error("fork test providers did not use their process-local effective configuration")
  else
    $spans
  end
