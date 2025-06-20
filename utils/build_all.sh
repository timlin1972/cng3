#! /bin/bash

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

while true
do
	cd "$script_dir"
	
	cd ..

	cd client
	npm install
	npm run build
	cd ..

	cd server
	$HOME/.cargo/bin/cargo run
	cd ..
done
