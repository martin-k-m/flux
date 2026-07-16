function greeting() {
  return 'Hello from a Flux-built Node app!'
}

module.exports = { greeting }

if (require.main === module) {
  console.log(greeting())
}
