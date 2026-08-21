<?php

require dirname(__DIR__) . '/vendor/autoload.php';

use Symfony\Component\EventDispatcher\EventDispatcher;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\HttpKernel\Controller\ArgumentResolver;
use Symfony\Component\HttpKernel\Controller\ControllerResolver;
use Symfony\Component\HttpKernel\HttpKernel;
use Symfony\Component\HttpKernel\HttpKernelInterface;

final class SymfonyFixtureController
{
    public function show(): Response
    {
        return new Response('Symfony OK');
    }
}

$request = Request::createFromGlobals();
$request->attributes->set('_route', 'users_show');
$request->attributes->set('_controller', SymfonyFixtureController::class . '::show');

$kernel = new HttpKernel(
    new EventDispatcher(),
    new ControllerResolver(),
    null,
    new ArgumentResolver()
);
$response = $kernel->handle($request);

$subRequest = Request::create('/fragment');
$subRequest->attributes->set('_route', 'fragment');
$subRequest->attributes->set('_controller', SymfonyFixtureController::class . '::show');
$kernel->handle($subRequest, HttpKernelInterface::SUB_REQUEST);

$response->send();
$kernel->terminate($request, $response);
