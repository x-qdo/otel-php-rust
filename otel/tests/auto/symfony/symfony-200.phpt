--TEST--
Test Symfony route instrumentation
--EXTENSIONS--
otel
--FILE--
<?php
include dirname(__DIR__, 2) . '/run-server.php';

$options = [
    'http' => [
        'method' => 'GET',
    ],
];

run_server('auto/symfony/public/index.php', $options, 'users/42');
?>
--EXPECTF--
==== Response ====
Symfony OK==== Server Output ====%A
Spans
Resource
%A
Span #0
	Instrumentation Scope
		Name         : "php:rinit"

	Name         : GET users_show
	TraceId      : %s
	SpanId       : %s
	TraceFlags   : TraceFlags(1)
	ParentSpanId : None (root span)
	Kind         : Server
	Start time   : %s
	End time     : %s
	Status       : Unset
	Attributes:
		 ->  url.full: String(Owned("/users/42"))
		 ->  http.request.method: String(Owned("GET"))
		 ->  php.framework.name: String(Static("symfony"))
		 ->  http.route: String(Owned("users_show"))
		 ->  php.framework.controller.name: String(Owned("SymfonyFixtureController"))
		 ->  php.framework.action.name: String(Owned("show"))
		 ->  http.response.status_code: I64(200)%A
