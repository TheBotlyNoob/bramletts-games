import client from "$lib/apolloClient.ts";
import type {
        ApolloQueryResult, ObservableQuery, WatchQueryOptions, QueryOptions, MutationOptions
      } from "@apollo/client";
import { readable } from "svelte/store";
import type { Readable } from "svelte/store";
import gql from "graphql-tag"
export type Maybe<T> = T | null;
export type InputMaybe<T> = Maybe<T>;
export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };
export type MakeOptional<T, K extends keyof T> = Omit<T, K> & { [SubKey in K]?: Maybe<T[SubKey]> };
export type MakeMaybe<T, K extends keyof T> = Omit<T, K> & { [SubKey in K]: Maybe<T[SubKey]> };
export type MakeEmpty<T extends { [key: string]: unknown }, K extends keyof T> = { [_ in K]?: never };
export type Incremental<T> = T | { [P in keyof T]?: P extends ' $fragmentName' | '__typename' ? T[P] : never };
/** All built-in and custom scalars, mapped to their actual values */
export type Scalars = {
  ID: { input: string; output: string; }
  String: { input: string; output: string; }
  Boolean: { input: boolean; output: boolean; }
  Int: { input: number; output: number; }
  Float: { input: number; output: number; }
  GameId: { input: any; output: any; }
};

export enum FirefoxStatus {
  Downloading = 'DOWNLOADING',
  Ready = 'READY'
}

export type GraphQlGame = {
  __typename?: 'GraphQLGame';
  icon: Scalars['String']['output'];
  id: Scalars['Int']['output'];
  name: Scalars['String']['output'];
  status: GraphQlGameStatus;
};

export type GraphQlGameStatus = {
  __typename?: 'GraphQLGameStatus';
  /** Progress in megabytes */
  progress?: Maybe<Array<Scalars['Int']['output']>>;
  status: GraphQlGameStatusInner;
};

export enum GraphQlGameStatusInner {
  Downloading = 'DOWNLOADING',
  Installing = 'INSTALLING',
  NotDownloaded = 'NOT_DOWNLOADED',
  Ready = 'READY',
  Running = 'RUNNING'
}

export type Mutation = {
  __typename?: 'Mutation';
  delete: VoidEnum;
  download: VoidEnum;
  launchFirefox: FirefoxStatus;
  run: VoidEnum;
  updateGameList: VoidEnum;
};


export type MutationDeleteArgs = {
  game: Scalars['GameId']['input'];
};


export type MutationDownloadArgs = {
  game: Scalars['GameId']['input'];
};


export type MutationRunArgs = {
  game: Scalars['GameId']['input'];
};

export type Query = {
  __typename?: 'Query';
  firefox: FirefoxStatus;
  game?: Maybe<GraphQlGame>;
  games: Array<GraphQlGame>;
};


export type QueryGameArgs = {
  id: Scalars['Int']['input'];
};

export enum VoidEnum {
  Void = 'VOID'
}

export type DeleteGameMutationVariables = Exact<{
  game: Scalars['GameId']['input'];
}>;


export type DeleteGameMutation = { __typename?: 'Mutation', delete: VoidEnum };

export type DownloadGameMutationVariables = Exact<{
  game: Scalars['GameId']['input'];
}>;


export type DownloadGameMutation = { __typename?: 'Mutation', download: VoidEnum };

export type RunGameMutationVariables = Exact<{
  game: Scalars['GameId']['input'];
}>;


export type RunGameMutation = { __typename?: 'Mutation', run: VoidEnum };

export type UpdateGamesMutationVariables = Exact<{ [key: string]: never; }>;


export type UpdateGamesMutation = { __typename?: 'Mutation', updateGameList: VoidEnum };

export type LaunchFirefoxMutationVariables = Exact<{ [key: string]: never; }>;


export type LaunchFirefoxMutation = { __typename?: 'Mutation', launchFirefox: FirefoxStatus };

export type GamesQueryVariables = Exact<{ [key: string]: never; }>;


export type GamesQuery = { __typename?: 'Query', games: Array<{ __typename?: 'GraphQLGame', id: number, name: string, icon: string, status: { __typename?: 'GraphQLGameStatus', status: GraphQlGameStatusInner, progress?: Array<number> | null } }> };


export const DeleteGameDoc = gql`
    mutation DeleteGame($game: GameId!) {
  delete(game: $game)
}
    `;
export const DownloadGameDoc = gql`
    mutation DownloadGame($game: GameId!) {
  download(game: $game)
}
    `;
export const RunGameDoc = gql`
    mutation RunGame($game: GameId!) {
  run(game: $game)
}
    `;
export const UpdateGamesDoc = gql`
    mutation UpdateGames {
  updateGameList
}
    `;
export const LaunchFirefoxDoc = gql`
    mutation LaunchFirefox {
  launchFirefox
}
    `;
export const GamesDoc = gql`
    query Games {
  games {
    id
    name
    icon
    status {
      status
      progress
    }
  }
}
    `;
export const DeleteGame = (
            options: Omit<
              MutationOptions<any, DeleteGameMutationVariables>, 
              "mutation"
            >
          ) => {
            const m = client.mutate<DeleteGameMutation, DeleteGameMutationVariables>({
              mutation: DeleteGameDoc,
              ...options,
            });
            return m;
          }
export const DownloadGame = (
            options: Omit<
              MutationOptions<any, DownloadGameMutationVariables>, 
              "mutation"
            >
          ) => {
            const m = client.mutate<DownloadGameMutation, DownloadGameMutationVariables>({
              mutation: DownloadGameDoc,
              ...options,
            });
            return m;
          }
export const RunGame = (
            options: Omit<
              MutationOptions<any, RunGameMutationVariables>, 
              "mutation"
            >
          ) => {
            const m = client.mutate<RunGameMutation, RunGameMutationVariables>({
              mutation: RunGameDoc,
              ...options,
            });
            return m;
          }
export const UpdateGames = (
            options: Omit<
              MutationOptions<any, UpdateGamesMutationVariables>, 
              "mutation"
            >
          ) => {
            const m = client.mutate<UpdateGamesMutation, UpdateGamesMutationVariables>({
              mutation: UpdateGamesDoc,
              ...options,
            });
            return m;
          }
export const LaunchFirefox = (
            options: Omit<
              MutationOptions<any, LaunchFirefoxMutationVariables>, 
              "mutation"
            >
          ) => {
            const m = client.mutate<LaunchFirefoxMutation, LaunchFirefoxMutationVariables>({
              mutation: LaunchFirefoxDoc,
              ...options,
            });
            return m;
          }
export const Games = (
            options: Omit<
              WatchQueryOptions<GamesQueryVariables>, 
              "query"
            >
          ): Readable<
            ApolloQueryResult<GamesQuery> & {
              query: ObservableQuery<
                GamesQuery,
                GamesQueryVariables
              >;
            }
          > => {
            const q = client.watchQuery({
              query: GamesDoc,
              ...options,
            });
            var result = readable<
              ApolloQueryResult<GamesQuery> & {
                query: ObservableQuery<
                  GamesQuery,
                  GamesQueryVariables
                >;
              }
            >(
              { data: {} as any, loading: true, error: undefined, networkStatus: 1, query: q },
              (set) => {
                q.subscribe((v: any) => {
                  set({ ...v, query: q });
                });
              }
            );
            return result;
          }
        
              export const AsyncGames = (
                options: Omit<
                  QueryOptions<GamesQueryVariables>,
                  "query"
                >
              ) => {
                return client.query<GamesQuery>({query: GamesDoc, ...options})
              }
            