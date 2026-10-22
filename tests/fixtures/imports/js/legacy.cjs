const stripe = require("stripe");
const tracing = require("@sentry/node/tracing");
const local = require("./util.js");

function charge(amount) {
  return stripe(process.env.KEY).charges.create({ amount: local.formatAmount(amount) });
}

module.exports = { charge, tracing };
