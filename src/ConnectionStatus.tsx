import { HStack, Icon, Text } from "@chakra-ui/react";
import { VscCircleFilled } from "react-icons/vsc";
import type { ConnectionStatus as Status } from "./components/Editor";


type ConnectionStatusProps = {
  connection: Status;
};

function ConnectionStatus({ connection }: ConnectionStatusProps) {
  return (
    <HStack gap={1} px={2}>
      <Icon
        as={VscCircleFilled}
        color={
          {
            connected: "green.500",
            disconnected: "orange.500",
            desynchronized: "red.500",
          }[connection]
        }
      />
      <Text fontSize="sm" fontStyle="italic">
        {
          {
            connected: "Connected!",
            disconnected: "Connecting...",
            desynchronized: "Disconnected, please refresh.",
          }[connection]
        }
      </Text>
    </HStack>
  );
}

export default ConnectionStatus;
