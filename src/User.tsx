import {
  Button,
  HStack,
  Icon,
  Input,
  Popover,
  Portal,
  Text,
} from "@chakra-ui/react";
import { useRef } from "react";
import { FaPalette } from "react-icons/fa";
import { VscAccount } from "react-icons/vsc";

import { UserInfo } from "./rustpad";

type UserProps = {
  info: UserInfo;
  onChangeName?: (name: string) => void;
  onChangeColor?: () => void;
};

export function User({ info }: UserProps) {
  const nameColor = `hsl(${info.hue}, 90%, 75%)`;
  return (
    <HStack gap={2}>
      <Icon as={VscAccount} color={nameColor} />
      <Text fontWeight="semibold" color={nameColor}>{info.name}</Text>
    </HStack>
  );
}

function UserMe({
  info,
  onChangeName,
  onChangeColor,
}: UserProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const nameColor = `hsl(${info.hue}, 90%, 75%)`;

  return (
    <Popover.Root initialFocusEl={() => inputRef.current}>
      <Popover.Trigger asChild>
        <Button variant="outline" size="xs">
          <Icon as={VscAccount} />
          <Text fontWeight="semibold" color={nameColor}>{info.name}</Text>
        </Button>
      </Popover.Trigger>
      <Portal>
        <Popover.Positioner>
          <Popover.Content>
            <Popover.Arrow />
            <Popover.Body>
              <Popover.Title fontWeight="semibold">
                Update Info
              </Popover.Title>
              <Input
                ref={inputRef}
                mb={2}
                value={info.name}
                maxLength={25}
                onChange={(event) => onChangeName?.(event.target.value)}
              />
              <Button size="sm" w="100%" onClick={onChangeColor}>
                <FaPalette /> Change Color
              </Button>
            </Popover.Body>
            <Popover.CloseTrigger />
          </Popover.Content>
        </Popover.Positioner>
      </Portal>
    </Popover.Root>
  );
}

export default UserMe;
