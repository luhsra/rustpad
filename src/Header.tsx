import { Box, Flex, HStack, Icon, IconButton, Text } from "@chakra-ui/react";
import ConnectionStatus from "./ConnectionStatus";
import { ConnectionState } from "./App";
import { VscColorMode } from "react-icons/vsc";

export type HeaderProps = {
    toggleColorMode: () => void;
    version: string;
    connection: ConnectionState,
}

function Header({ toggleColorMode, version, connection }: HeaderProps) {
    return (
        <Flex flexShrink={0}>
            <HStack px={2} flexShrink={0} fontSize="sm">
                <Text>SRApad ({version})</Text>
            </HStack>

            <Box flex={1}></Box>

            <ConnectionStatus connection={connection} />
            <IconButton size="xs" variant="outline" aria-label="Dark Mode" onClick={toggleColorMode}>
                <Icon as={VscColorMode} />
            </IconButton>
        </Flex>
    );
}

export default Header;
