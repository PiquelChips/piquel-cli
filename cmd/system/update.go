package system

import (
	"log"

	"github.com/spf13/cobra"
)

var (
	updateCmd = &cobra.Command{
		Use:   "update",
		Short: "Will update your system",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if verboseFlag {
				log.Printf("Verbose!\n")
			}

			return nil
		},
	}
	verboseFlag bool
)

func init() {
	SystemCmd.AddCommand(updateCmd)

	updateCmd.Flags().BoolVarP(&verboseFlag, "verbose", "v", false, "verbose output")
}
