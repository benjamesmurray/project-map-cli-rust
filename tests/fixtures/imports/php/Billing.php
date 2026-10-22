<?php

namespace App\Billing;

use Stripe\StripeClient;
use Sentry\State\Scope;
use App\Support\Money;

class Billing
{
    public function client(string $key): StripeClient
    {
        return new StripeClient($key);
    }
}
