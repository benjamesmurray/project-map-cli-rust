import json, sentry_sdk
import sentry_sdk.integrations
from sentry_sdk.integrations.django import DjangoIntegration
from . import helpers


def configure(dsn):
    sentry_sdk.init(dsn=dsn, integrations=[DjangoIntegration()], before_send=helpers.scrub)
    return json.dumps({"dsn": dsn})
