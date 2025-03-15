p:
	@go run main.go load $(shell go run main.go list | fzf)
