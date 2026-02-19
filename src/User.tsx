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
import { VscUnverified, VscVerified, VscVerifiedFilled } from "react-icons/vsc";

import { type OnlineUser } from "./rustpad";

type UserProps = {
  info: OnlineUser;
  onChangeName?: (name: string) => void;
  onChangeColor?: () => void;
};

const icons = {
  admin: VscVerifiedFilled,
  user: VscVerified,
  anon: VscUnverified,
}

export function User({ info }: UserProps) {
  const nameColor = `hsl(${info.hue}, 90%, 75%)`;
  const icon = icons[info.role];
  const name = info.role !== "anon" ? info.name : ("Anon " + info.name);

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

  const icon = icons[info.role];
  const name = info.role !== "anon" ? info.name : ("Anon " + info.name);

  const query = new URLSearchParams({ redirect: location.hash.slice(1) });
  const login_url = "/auth/login?" + query.toString();
  const logout_url = "/auth/logout?" + query.toString();

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
                disabled={info.role !== "anon"}
                onChange={(event) => onChangeName?.(event.target.value)}
              />
              <Button size="sm" w="100%" onClick={onChangeColor}>
                <FaPalette /> Change Color
              </Button>
              {info.role !== "anon" ? (
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
