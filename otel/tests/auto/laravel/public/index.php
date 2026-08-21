<?php

require dirname(__DIR__) . '/vendor/autoload.php';

use Illuminate\Contracts\Http\Kernel as KernelContract;
use Illuminate\Http\Request;
use Illuminate\Http\Response;
use Illuminate\Routing\Route;

final class LaravelFixtureKernel implements KernelContract
{
    public function bootstrap(): void
    {
    }

    public function handle($request): Response
    {
        $route = new Route(
            ['GET'],
            'users/{user}',
            [
                'uses' => 'App\\Http\\Controllers\\UserController@show',
                'controller' => 'App\\Http\\Controllers\\UserController@show',
            ]
        );
        $request->setRouteResolver(static fn (): Route => $route);

        return new Response('Laravel OK');
    }

    public function terminate($request, $response): void
    {
    }

    public function getApplication()
    {
        return null;
    }
}

$request = Request::capture();
$kernel = new LaravelFixtureKernel();
$response = $kernel->handle($request);
$response->send();
$kernel->terminate($request, $response);
