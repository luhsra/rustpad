import { Box, Flex, HStack, Icon, IconButton, Text } from "@chakra-ui/react";
import { VscAdd, VscColorMode } from "react-icons/vsc";

import ConnectionStatus from "./ConnectionStatus";
import type { ConnectionStatus as Status } from "./components/Editor";

export type HeaderProps = {
  toggleColorMode: () => void;
  version: string;
  connection: Status;
};

function Header({ toggleColorMode, version, connection }: HeaderProps) {
  return (
    <Flex flexShrink={0}>
      <HStack px={2} flexShrink={0} fontSize="sm">
        <Text>SRApad ({version})</Text>
      </HStack>

      <Box flex={1}></Box>

      <ConnectionStatus connection={connection} />
      <IconButton
        size="xs"
        variant="outline"
        aria-label="Dark Mode"
        onClick={toggleColorMode}
      >
        <Icon as={VscColorMode} />
      </IconButton>
    </Flex>
  );
}

export default Header;
