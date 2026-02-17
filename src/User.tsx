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
import { VscWorkspaceTrusted, VscWorkspaceUnknown } from "react-icons/vsc";

import { type UserInfo } from "./rustpad";

type UserProps = {
  info: UserInfo;
  onChangeName?: (name: string) => void;
  onChangeColor?: () => void;
};

export function User({ info }: UserProps) {
  const nameColor = `hsl(${info.hue}, 90%, 75%)`;
  let icon = info.admin ? VscWorkspaceTrusted : VscWorkspaceUnknown;
  let name = info.admin ? info.name : ("Anon " + info.name);

  return (
    <HStack gap={2}>
      <Icon as={icon} color={nameColor} />
      <Text fontWeight="semibold" color={nameColor}>
        {name}
      </Text>
    </HStack>
  );
}

function UserMe({ info, onChangeName, onChangeColor }: UserProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const nameColor = `hsl(${info.hue}, 90%, 75%)`;

  let icon = info.admin ? VscWorkspaceTrusted : VscWorkspaceUnknown;
  let name = info.admin ? info.name : ("Anon " + info.name);

  let query = new URLSearchParams({ location: location.hash.slice(1) });
  let login_url = "/auth/login?" + query.toString();
  let logout_url = "/auth/logout?" + query.toString();

  return (
    <Popover.Root initialFocusEl={() => inputRef.current}>
      <Popover.Trigger asChild>
        <Button variant="outline" size="xs">
          <Icon as={icon} color={nameColor} />
          <Text fontWeight="semibold" color={nameColor}>
            {name}
          </Text>
        </Button>
      </Popover.Trigger>
      <Portal>
        <Popover.Positioner>
          <Popover.Content>
            <Popover.Arrow />
            <Popover.Body>
              <Popover.Title fontWeight="semibold">Update Info</Popover.Title>
              <Input
                ref={inputRef}
                mb={2}
                value={info.name}
                maxLength={25}
                disabled={info.admin}
                onChange={(event) => onChangeName?.(event.target.value)}
              />
              <Button size="sm" w="100%" onClick={onChangeColor}>
                <FaPalette /> Change Color
              </Button>
              {info.admin ? (
                <Button mt={2} size="sm" w="100%" colorPalette="red" onClick={() => (location.href = logout_url)}>
                  Logout
                </Button>
              ) : (
                <Button mt={2} size="sm" w="100%" colorPalette="green" onClick={() => (location.href = login_url)}>
                  Login
                </Button>
              )}
            </Popover.Body>
            <Popover.CloseTrigger />
          </Popover.Content>
        </Popover.Positioner>
      </Portal>
    </Popover.Root>
  );
}

export default UserMe;
